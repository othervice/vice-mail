import {
  Archive,
  BookOpen,
  Bookmark,
  Clock,
  FilePen,
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
  pinned: Pin,
  sent: Send,
  drafts: FilePen,
  archive: Archive,
  trash: Trash,
  folders: Folder,
  savedViews: Bookmark,
};
