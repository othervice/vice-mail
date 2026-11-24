import {
  Archive,
  BookOpen,
  BookUser,
  Bookmark,
  Clock,
  FilePen,
  Flag,
  Folder,
  Inbox,
  ListTodo,
  Paperclip,
  Pin,
  Send,
  Trash,
  type LucideIcon,
} from "lucide-react";
import type { MailViewId } from "../../stores/ui";

export const MAIL_VIEW_ICONS: Record<MailViewId, LucideIcon> = {
  inbox: Inbox,
  later: Clock,
  tasks: ListTodo,
  readingList: BookOpen,
  pinnedAttachments: Paperclip,
  flagged: Flag,
  pinned: Pin,
  sent: Send,
  drafts: FilePen,
  archive: Archive,
  trash: Trash,
  folders: Folder,
  savedViews: Bookmark,
  // Stage 17 (decision 0050).
  contacts: BookUser,
};
