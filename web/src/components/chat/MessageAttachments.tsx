import { FileTextIcon } from "lucide-react";
import {
  Attachment,
  AttachmentContent,
  AttachmentDescription,
  AttachmentGroup,
  AttachmentMedia,
  AttachmentTitle,
} from "@/components/ui/attachment";
import type { AttachmentMeta } from "@/contracts/session-protocol";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function MessageAttachments({
  ids,
  attachmentMap,
}: {
  ids: string[];
  attachmentMap: Map<string, AttachmentMeta>;
}) {
  if (ids.length === 0) {
    return null;
  }
  return (
    <AttachmentGroup className="mb-1.5 justify-end">
      {ids.map((attId) => {
        const meta = attachmentMap.get(attId);
        return (
          <Attachment key={attId} size="xs">
            <AttachmentMedia variant="icon">
              <FileTextIcon className="size-3.5" />
            </AttachmentMedia>
            <AttachmentContent>
              <AttachmentTitle>{meta?.filename ?? attId}</AttachmentTitle>
              {meta && (
                <AttachmentDescription>
                  {formatBytes(meta.size_bytes)}
                </AttachmentDescription>
              )}
            </AttachmentContent>
          </Attachment>
        );
      })}
    </AttachmentGroup>
  );
}
