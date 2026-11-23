//! V-02: a key belongs to the server it was entered for. Saving a profile with another server
//! (scheme, host or port) and no new key removes the stored key, keychain first; the model list
//! and the connection test then build the provider for the new server without any key. A
//! change of the model or the path keeps the key.

use super::*;
use crate::credentials::SecretOp;

/// Base URL and key of every provider the core builds, in order.
type Built = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Like [`route_to`], recording what each provider was built with.
fn recording(mock: &Mock, built: &Built) -> Arc<crate::ai::ProviderFactory> {
    let inner = route_to(mock);
    let built = Arc::clone(built);
    Arc::new(move |config: ProviderConfig| {
        lock(&built).push((
            config.base_url.clone(),
            config
                .api_key
                .as_ref()
                .map(|key| key.expose_secret().clone()),
        ));
        inner(config)
    })
}

fn input(id: &str, url: &str, model: &str, key: AiKeyChange) -> AiProfileInput {
    AiProfileInput {
        id: Some(id.to_owned()),
        name: "Test".into(),
        kind: AiProviderKind::OpenaiCompatible,
        base_url: url.into(),
        model: model.into(),
        structured_output: true,
        max_output_tokens: None,
        key,
    }
}

fn set_key() -> AiKeyChange {
    AiKeyChange::Set { key: KEY.into() }
}

/// Runs the model list and the connection test of `profile` and returns what the providers
/// were built with.
async fn list_and_test(h: &Harness, built: &Built, profile: &str) -> Vec<(String, Option<String>)> {
    lock(built).clear();
    h.core
        .list_ai_models(profile.to_owned())
        .await
        .expect("models");
    h.core
        .test_ai_connection(profile.to_owned())
        .await
        .expect("test");
    lock(built).clone()
}

#[tokio::test]
async fn another_server_removes_the_kept_key_before_anything_is_sent() {
    let h = Harness::new().await;
    let built = Built::default();
    h.core.set_ai_provider_factory(recording(&h.mock, &built));
    let profile = h.profile("https://ai.example.test/v1").await;
    h.core.set_ai_enabled(true).await.expect("on");
    let entry = CredentialRef::for_ai_profile(&profile);
    for (before, after) in [
        // Host.
        (
            "https://ai.example.test/v1",
            "https://other.example.test/v1",
        ),
        (
            "https://ai.example.test/v1",
            "https://ai.example.test.other.example/v1",
        ),
        // Port.
        (
            "https://ai.example.test/v1",
            "https://ai.example.test:8443/v1",
        ),
        // Scheme.
        ("http://localhost:8080/v1", "https://localhost:8080/v1"),
    ] {
        h.core
            .save_ai_profile(input(&profile, before, "test-model", set_key()))
            .await
            .expect("key stored");
        // The key goes to its own server ...
        let seen = list_and_test(&h, &built, &profile).await;
        assert_eq!(seen.len(), 2, "{before}");
        assert!(
            seen.iter()
                .all(|(url, key)| url == before && key.as_deref() == Some(KEY)),
            "{before}"
        );

        // ... and is removed when the profile moves without a new one.
        let settings = h
            .core
            .save_ai_profile(input(&profile, after, "test-model", AiKeyChange::Keep))
            .await
            .expect("moved");
        let info = &settings.profiles[0];
        assert_eq!(info.base_url, after);
        assert!(!info.has_key, "{after}");
        assert!(info.key_removed_on_server_change, "{after}");
        assert!(!h.keychain.contains(&entry), "{after}");
        assert!(
            h.keychain
                .secrets()
                .iter()
                .all(|secret| secret.expose_secret() != KEY),
            "{after}"
        );
        // Neither the model list nor the connection test sends it to the new server.
        let seen = list_and_test(&h, &built, &profile).await;
        assert_eq!(seen.len(), 2, "{after}");
        assert!(
            seen.iter().all(|(url, key)| url == after && key.is_none()),
            "{after}: {:?}",
            seen.iter()
                .map(|(_, key)| key.is_some())
                .collect::<Vec<_>>()
        );
        // The notice stays until the profile is saved again.
        let settings = h.core.get_ai_settings().await.expect("settings");
        assert!(settings.profiles[0].key_removed_on_server_change);
    }

    // Entering the key again stores it for the new server and ends the notice.
    let settings = h
        .core
        .save_ai_profile(input(
            &profile,
            "https://localhost:8080/v1",
            "test-model",
            set_key(),
        ))
        .await
        .expect("key again");
    assert!(settings.profiles[0].has_key);
    assert!(!settings.profiles[0].key_removed_on_server_change);
    let seen = list_and_test(&h, &built, &profile).await;
    assert!(
        seen.iter()
            .all(|(url, key)| url == "https://localhost:8080/v1" && key.as_deref() == Some(KEY))
    );

    // A new server with a new key: the new key, no notice.
    let settings = h
        .core
        .save_ai_profile(input(
            &profile,
            "https://third.example.test/v1",
            "test-model",
            AiKeyChange::Set {
                key: "sk-other-key".into(),
            },
        ))
        .await
        .expect("new server, new key");
    assert!(settings.profiles[0].has_key);
    assert!(!settings.profiles[0].key_removed_on_server_change);
    let seen = list_and_test(&h, &built, &profile).await;
    assert!(
        seen.iter()
            .all(|(url, key)| url == "https://third.example.test/v1"
                && key.as_deref() == Some("sk-other-key"))
    );

    // A profile without a key that moves has nothing to remove and no notice.
    let settings = h
        .core
        .save_ai_profile(input(
            &profile,
            "https://third.example.test/v1",
            "test-model",
            AiKeyChange::Remove,
        ))
        .await
        .expect("key removed by the user");
    assert!(!settings.profiles[0].key_removed_on_server_change);
    let settings = h
        .core
        .save_ai_profile(input(
            &profile,
            "https://fourth.example.test/v1",
            "test-model",
            AiKeyChange::Keep,
        ))
        .await
        .expect("moved without a key");
    assert!(!settings.profiles[0].has_key);
    assert!(!settings.profiles[0].key_removed_on_server_change);
}

#[tokio::test]
async fn a_model_or_path_change_keeps_the_key() {
    let h = Harness::new().await;
    let built = Built::default();
    h.core.set_ai_provider_factory(recording(&h.mock, &built));
    let profile = h.profile("https://ai.example.test/v1").await;
    h.core.set_ai_enabled(true).await.expect("on");
    let entry = CredentialRef::for_ai_profile(&profile);
    for (url, model) in [
        ("https://ai.example.test/v1", "other"),
        ("https://ai.example.test/api/v2", "other"),
        // The same origin, written differently: case of the host, the scheme's default port.
        ("https://AI.Example.Test:443/api/v2", "test-model"),
    ] {
        let settings = h
            .core
            .save_ai_profile(input(&profile, url, model, AiKeyChange::Keep))
            .await
            .expect("saved");
        let info = &settings.profiles[0];
        assert_eq!((info.base_url.as_str(), info.model.as_str()), (url, model));
        assert!(info.has_key, "{url}");
        assert!(!info.key_removed_on_server_change, "{url}");
        let stored = h.keychain.get(&entry).expect("keychain").expect("entry");
        assert_eq!(stored.expose_secret(), KEY);
        let seen = list_and_test(&h, &built, &profile).await;
        assert!(
            seen.iter()
                .all(|(seen, key)| seen == url && key.as_deref() == Some(KEY)),
            "{url}"
        );
    }
}

#[tokio::test]
async fn a_failing_keychain_delete_refuses_the_save_and_changes_nothing() {
    let h = Harness::new().await;
    let built = Built::default();
    h.core.set_ai_provider_factory(recording(&h.mock, &built));
    let profile = h.profile("https://ai.example.test/v1").await;
    h.core.set_ai_enabled(true).await.expect("on");
    let entry = CredentialRef::for_ai_profile(&profile);
    let before = h.core.get_ai_settings().await.expect("settings");
    assert!(before.profiles[0].has_key);

    h.keychain.set_failing(SecretOp::Delete, true);
    assert_eq!(
        code(
            h.core
                .save_ai_profile(input(
                    &profile,
                    "https://other.example.test/v1",
                    "other",
                    AiKeyChange::Keep,
                ))
                .await
        ),
        ErrorCode::Keychain
    );
    // Nothing changed: the profile still points at its server and has its key.
    assert_eq!(h.core.get_ai_settings().await.expect("settings"), before);
    let stored = h.keychain.get(&entry).expect("keychain").expect("entry");
    assert_eq!(stored.expose_secret(), KEY);
    let seen = list_and_test(&h, &built, &profile).await;
    assert!(
        seen.iter()
            .all(|(url, key)| url == "https://ai.example.test/v1" && key.as_deref() == Some(KEY))
    );

    // With the keychain working again the same save goes through.
    h.keychain.set_failing(SecretOp::Delete, false);
    let settings = h
        .core
        .save_ai_profile(input(
            &profile,
            "https://other.example.test/v1",
            "other",
            AiKeyChange::Keep,
        ))
        .await
        .expect("saved");
    assert!(!settings.profiles[0].has_key);
    assert!(settings.profiles[0].key_removed_on_server_change);
    assert!(!h.keychain.contains(&entry));
}
