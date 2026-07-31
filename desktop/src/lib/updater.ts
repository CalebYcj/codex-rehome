import { getVersion } from "@tauri-apps/api/app";
import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";

export type UpdateCheckResult =
  | { status: "unsupported"; currentVersion: null }
  | { status: "current"; currentVersion: string }
  | {
      status: "available";
      currentVersion: string;
      version: string;
      notes: string | null;
    };

export interface UpdateProgress {
  downloadedBytes: number;
  totalBytes: number | null;
  percent: number | null;
}

let checkedUpdate: Update | null = null;

export async function checkForUpdates(): Promise<UpdateCheckResult> {
  if (!isTauri()) return { status: "unsupported", currentVersion: null };

  const currentVersion = await getVersion();
  if (checkedUpdate) {
    await checkedUpdate.close().catch(() => undefined);
    checkedUpdate = null;
  }

  const update = await check({ timeout: 15_000 });
  if (!update) return { status: "current", currentVersion };

  checkedUpdate = update;
  return {
    status: "available",
    currentVersion,
    version: update.version,
    notes: update.body?.trim() || null,
  };
}

export async function installCheckedUpdate(
  onProgress: (progress: UpdateProgress) => void,
): Promise<void> {
  if (!checkedUpdate) throw new Error("请先检查更新");

  let downloadedBytes = 0;
  let totalBytes: number | null = null;
  await checkedUpdate.downloadAndInstall((event: DownloadEvent) => {
    if (event.event === "Started") {
      totalBytes = event.data.contentLength ?? null;
      downloadedBytes = 0;
    } else if (event.event === "Progress") {
      downloadedBytes += event.data.chunkLength;
    } else {
      downloadedBytes = totalBytes ?? downloadedBytes;
    }

    onProgress({
      downloadedBytes,
      totalBytes,
      percent:
        totalBytes && totalBytes > 0
          ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100))
          : null,
    });
  });
  await relaunch();
}
