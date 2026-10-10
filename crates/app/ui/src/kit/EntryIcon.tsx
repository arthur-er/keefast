import {
  AppWindow, Apple, Archive, Award, Ban, Banknote, BatteryWarning, Bird, Book, BookOpen, Camera,
  CircleCheck, ClipboardCheck, Clock, Disc, Feather, FileCheck, FileLock, FilePlus, FileQuestionMark,
  Flag, Folder, FolderArchive, FolderCheck, FolderOpen, Globe, Hammer, HardDrive, House, IdCard, Image,
  Info, Key, KeyRound, Landmark, LayoutGrid, List, LockOpen, type LucideIcon, Mail, Mailbox, MailSearch,
  MemoryStick, MessagesSquare, Monitor, Network, NotepadText, Package, Pen, Play, Plug, Printer, Puzzle,
  Radio, Save, Scan, Server, Settings, Smartphone, Sparkles, SquareTerminal, Star, StickyNote, Terminal,
  Trash, TriangleAlert, Tv, User, Wrench, Zap,
} from "lucide-react";

import type { IconView } from "@/bindings";
import { cn } from "@/lib/utils";

/** The Lucide icon drawn for each of KeePass's 69 standard icons, by number: the nearest in
 * meaning to KeePass's names (PwIcon), as the GPUI app drew them. */
export const STANDARD_ICONS: readonly LucideIcon[] = [
  Key, Globe, TriangleAlert, Server, FolderCheck, MessagesSquare, Puzzle, NotepadText, Plug, IdCard,
  FileCheck, Camera, Radio, KeyRound, Zap, Scan, Sparkles, Disc, Monitor, Mail, Settings,
  ClipboardCheck, FilePlus, Tv, BatteryWarning, Mailbox, Save, HardDrive, FileQuestionMark,
  SquareTerminal, Terminal, Printer, LayoutGrid, Play, Wrench, Network, Archive, Landmark, AppWindow,
  Clock, MailSearch, Flag, MemoryStick, Trash, StickyNote, Ban, Info, Package, Folder, FolderOpen,
  FolderArchive, LockOpen, FileLock, CircleCheck, Pen, Image, Book, List, User, Hammer, House, Star,
  Bird, Feather, Apple, BookOpen, Banknote, Award, Smartphone,
];

/** An Entry's picture: its site's favicon once downloaded, else its custom icon, else its
 * standard icon. */
export function EntryIcon({
  icon,
  faviconUrl,
  size = 16,
  className,
}: {
  icon: IconView;
  faviconUrl?: string | null;
  size?: number;
  className?: string;
}) {
  const src = faviconUrl ?? (icon.kind === "custom" ? icon.url : null);
  if (src) {
    return <img src={src} alt="" width={size} height={size} className={cn("flex-none rounded-xs", className)} />;
  }
  const number = icon.kind === "standard" ? icon.number : 0;
  const Icon = STANDARD_ICONS[number] ?? Key;
  return <Icon className={cn("text-fg-1", className)} style={{ width: size, height: size }} />;
}
