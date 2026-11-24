import { QueryClient, QueryClientContext, useQuery } from "@tanstack/react-query";
import { useContext } from "react";
import { contactsIpc, type ContactPictureRef } from "../../services/ipc";

/*
 * Stage 17 (decision 0050): which addresses have a picture, and the pictures themselves. Every
 * picture is a JPEG the core's helper wrote (never a file's own bytes, never from the network);
 * the UI only shows the data URL it is given. `contactsChanged` refreshes both (`refreshContacts`
 * invalidates the `contactPictures` prefix).
 */

export const contactPicturesQueryKey = ["contactPictures"] as const;
export const contactPictureQueryKey = (key: string, revision: number) => ["contactPictures", "picture", key, revision] as const;

/** Without a query client (a component rendered on its own in a test) nothing is asked. */
const detached = new QueryClient();

/** An address as the picture list writes it: lower case, without surrounding space. */
export function pictureEmailKey(email: string): string {
  return email.trim().toLowerCase();
}

function indexOf(refs: readonly ContactPictureRef[]): ReadonlyMap<string, ContactPictureRef> {
  const index = new Map<string, ContactPictureRef>();
  for (const ref of refs) for (const email of ref.emails) index.set(pictureEmailKey(email), ref);
  return index;
}

/**
 * The picture of the contact with this address as a data URL, or `null` (no picture, still
 * loading, or no core to ask). One list for the whole window, then one request per picture
 * revision, both kept until a change: rows of the message list cost a map lookup.
 */
export function useContactPicture(email: string | null | undefined): string | null {
  const client = useContext(QueryClientContext);
  const available = client !== undefined;
  const index = useQuery(
    {
      queryKey: contactPicturesQueryKey,
      queryFn: () => contactsIpc.listContactPictures(),
      select: indexOf,
      staleTime: Infinity,
      retry: false,
      enabled: available,
    },
    client ?? detached,
  );
  const ref = email ? index.data?.get(pictureEmailKey(email)) : undefined;
  const picture = useQuery(
    {
      queryKey: contactPictureQueryKey(ref?.key ?? "", ref?.revision ?? 0),
      queryFn: () => contactsIpc.getContactPicture(ref?.key ?? ""),
      staleTime: Infinity,
      retry: false,
      enabled: available && ref !== undefined,
    },
    client ?? detached,
  );
  return ref ? (picture.data?.dataUrl ?? null) : null;
}
