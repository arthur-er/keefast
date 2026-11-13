// Entry editor A's Attachments: add, preview inline, save and remove. Attachments have commands
// of their own, so each change applies to the Entry at once (keeping the version it replaces
// in History) rather than waiting for "Done".

import { useState } from "react";
import { File, FileImage, FileKey, FileText, Paperclip, Plus, Trash2 } from "lucide-react";
import { useQuery } from "@tanstack/react-query";

import { api, errorMessage } from "@/api";
import type { AttachmentView, EntryDetail } from "@/bindings";
import { Button, toast } from "@/kit";
import { formatSize } from "@/lib/format";

function fileIcon(attachment: AttachmentView) {
  if (attachment.preview === "image") return FileImage;
  if (attachment.preview === "text") return FileText;
  return /\.(pub|key|pem|asc)$|^id_/i.test(attachment.name) ? FileKey : File;
}

/** `entry` is `null` for a new Entry, which has nowhere to attach files to yet. `applyNow` runs
 * the changes, which apply to the Entry at once, for the editor to know they are its own. */
export function Attachments({
  entry,
  applyNow,
}: {
  entry: EntryDetail | null;
  applyNow: <T>(run: () => Promise<T>) => Promise<T>;
}) {
  const [previewing, setPreviewing] = useState<number | null>(null);
  const attachments = entry?.attachments ?? [];
  const add = async () => {
    if (!entry) return;
    try {
      const added = await applyNow(() => api.addAttachments(entry.vault, entry.id));
      if (added.added.length) {
        toast(added.added.length === 1 ? `Attached ${added.added[0]}` : `Attached ${added.added.length} files`, { icon: Paperclip });
      }
      for (const error of added.errors) toast(error, { icon: "error" });
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  const remove = async (attachment: AttachmentView) => {
    if (!entry) return;
    try {
      await applyNow(() => api.removeAttachment(entry.vault, entry.id, attachment.index));
      setPreviewing(null);
      toast(<>Removed {attachment.name}<span className="subtle"> · the earlier version keeps it in History</span></>, { icon: Trash2 });
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  const save = async (attachment: AttachmentView) => {
    if (!entry) return;
    try {
      const path = await api.saveAttachment(entry.vault, entry.id, attachment.index);
      if (path) toast(`Saved ${attachment.name} to ${path}.`);
    } catch (error) {
      toast(errorMessage(error), { icon: "error" });
    }
  };
  return (
    <div className="ee-sec">
      <div className="ee-sec-head">
        <h3>Attachments</h3>
        <span className="n">{attachments.length}</span>
        <Button variant="ghost" size="sm" disabled={!entry} title={entry ? "Attach files" : "Files can be attached once the Entry is created"} onClick={() => void add()}>
          <Plus /> File
        </Button>
      </div>
      {attachments.length === 0 ? (
        <div className="ee-empty">{entry ? "No attachments." : "Files can be attached once the Entry is created."}</div>
      ) : (
        attachments.map((attachment) => {
          const Icon = fileIcon(attachment);
          const open = previewing === attachment.index;
          return (
            <div key={`${attachment.index}:${attachment.name}`}>
              <div className="ee-att">
                <span className="ee-att-ic">
                  <Icon />
                </span>
                <span className="nm truncate">{attachment.name}</span>
                <span className="sz">{formatSize(attachment.size)}</span>
                <span className="ee-att-acts">
                  {attachment.preview !== "none" && (
                    <Button variant="ghost" size="sm" aria-expanded={open} onClick={() => setPreviewing(open ? null : attachment.index)}>
                      {open ? "Hide preview" : "Preview"}
                    </Button>
                  )}
                  <Button variant="ghost" size="sm" onClick={() => void save(attachment)}>
                    Save…
                  </Button>
                  <Button variant="dangerGhost" size="sm" onClick={() => void remove(attachment)}>
                    Remove
                  </Button>
                </span>
              </div>
              {open && entry && <Preview entry={entry} attachment={attachment} />}
            </div>
          );
        })
      )}
    </div>
  );
}

function Preview({ entry, attachment }: { entry: EntryDetail; attachment: AttachmentView }) {
  const preview = useQuery({
    queryKey: ["attachmentPreview", entry.vault, entry.id, attachment.index, attachment.name],
    queryFn: () => api.previewAttachment(entry.vault, entry.id, attachment.index),
    staleTime: 0,
    gcTime: 0,
  });
  const content = preview.data;
  if (preview.error) return <div className="ee-att-prev text-danger">{errorMessage(preview.error)}</div>;
  if (!content) return null;
  return (
    <div className="ee-att-prev">
      {content.kind === "text" ? (
        <>
          {content.text}
          {content.cut && <span className="cut">Only the start is shown.</span>}
        </>
      ) : content.kind === "image" ? (
        <img src={content.dataUrl} alt={attachment.name} />
      ) : (
        <span className="cut">keefast can't show this file. Save it to open it.</span>
      )}
    </div>
  );
}
