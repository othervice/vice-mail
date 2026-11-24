# Documentation

## For users (English)

| File | Content |
|---|---|
| [INSTALL.md](INSTALL.md) | installing, Gatekeeper and SmartScreen warnings, updates, data locations, uninstalling |
| [PRIVACY.md](PRIVACY.md) | every connection the app makes, what is stored where, what is in the keychain |
| [PROVIDERS.md](PROVIDERS.md) | what works with which mail provider |
| [SYNC.md](SYNC.md) | the settings sync format and what the mail server can see |

## For contributors (English)

| File | Content |
|---|---|
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | how to set up, test and submit changes; the rules for code |
| [SETUP.md](SETUP.md) | setting up a development machine, every tool with its tested version |
| [ARCHITECTURE.md](ARCHITECTURE.md) | stack, module boundaries, data model, mail engine rules, security boundaries, allowed network connections |
| [SECURITY_MODEL.md](SECURITY_MODEL.md) | threat model: what is protected, what is not, where the boundaries are |
| [RELEASING.md](RELEASING.md) | building, signing and publishing a release; the update signing key |
| [brand/](brand/README.md) | logo, icon and colours |
| [images/](images/README.md) | screenshots for the README |

## Project log (German)

Vice Mail was planned and built in numbered stages. These documents were written in German while
the project was private and are kept as a record: they hold the reasoning, measurements and test
results behind the code. They are not needed to use or build the app.

| Path | Content |
|---|---|
| [PLAN.md](PLAN.md) | the stages, their order and status |
| [stages/](stages/) | one file per stage: goal, tasks, acceptance checks, what is out of scope, and a "Result" (*Ergebnis*) section with what was actually run and checked |
| [decisions/](decisions/) | recorded decisions (`NNNN-title.md`), each with reason, status, affected code and measurements |
| [SPEC.md](SPEC.md) | product description: principles, features, performance targets, what is not in version 1 |
| [DESIGN.md](DESIGN.md) | interface design: layout, tokens, interaction rules |
| [testing/](testing/) | manual test checklists (Windows, screen readers) and performance measurements |

`CLAUDE.md` at the top level holds the working rules for the coding sessions (written for Claude
Code). Its rules apply to every contributor and are restated in English in
[CONTRIBUTING.md](../CONTRIBUTING.md).

Folder for the maintainer: `internal/` (notes on preparing the publication, German).
