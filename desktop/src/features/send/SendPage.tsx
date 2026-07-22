import { useMemo, useState, type RefObject } from "react";
import {
  CheckCircle2,
  FileArchive,
  FolderOpen,
  Image,
  LoaderCircle,
  MessageSquareText,
  PackagePlus,
  Puzzle,
  Sparkles,
} from "lucide-react";

import {
  createPackage,
  inspectPackage,
  openPath,
  pickRehomeSavePath,
} from "../../lib/api";
import { errorMessage, type CodexInventory, type CreatePackageReport, type PackagePreview } from "../../lib/types";

interface SendPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
}

export default function SendPage({ headingRef, inventory }: SendPageProps) {
  const [projects, setProjects] = useState<Set<string>>(new Set());
  const [conversations, setConversations] = useState<Set<string>>(new Set());
  const [skills, setSkills] = useState(false);
  const [plugins, setPlugins] = useState(false);
  const [images, setImages] = useState(false);
  const [outputPath, setOutputPath] = useState<string | null>(null);
  const [report, setReport] = useState<CreatePackageReport | null>(null);
  const [packagePreview, setPackagePreview] = useState<PackagePreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hasContent = conversations.size > 0 || skills || plugins || images;
  const canCreate = Boolean(inventory && projects.size > 0 && hasContent && !busy);
  const projectOptions = useMemo(() => {
    if (!inventory) return [];
    if (inventory.projects.length) {
      return inventory.projects.map((project) => ({
        id: project.project_id,
        name: project.name,
        path: project.source_path,
        fileCount: project.file_count,
      }));
    }
    return inventory.project_paths.map((path) => ({
      id: path,
      name: pathName(path),
      path,
      fileCount: 0,
    }));
  }, [inventory]);
  const conversationOptions = useMemo(() => {
    if (!inventory) return [];
    if (inventory.conversations.length) return inventory.conversations;
    return inventory.conversation_paths.flatMap((path) => {
      const taskId = path.match(/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$/i)?.[1];
      if (!taskId) return [];
      return [{
        task_id: taskId,
        project_id: null,
        title: `对话 ${taskId.slice(0, 8)}`,
        updated_at: path,
        content_hash: "",
        archive_path: "",
      }];
    });
  }, [inventory]);

  function toggle(setter: (value: Set<string>) => void, current: Set<string>, value: string) {
    const next = new Set(current);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setter(next);
  }

  async function chooseOutput() {
    const chosen = await pickRehomeSavePath(outputPath ?? undefined);
    if (chosen) setOutputPath(chosen);
  }

  async function handleCreate() {
    if (!inventory || !canCreate) return;
    setError(null);
    setBusy(true);
    try {
      const destination = outputPath ?? (await pickRehomeSavePath());
      if (!destination) return;
      setOutputPath(destination);
      const created = await createPackage({
        codex_home: inventory.codex_home,
        project_paths: [...projects],
        conversation_ids: [...conversations],
        output_path: destination,
        source_device_id: inventory.source_device_id,
        include_skills: skills,
        include_plugins: plugins,
        include_generated_images: images,
      });
      setReport(created);
      setPackagePreview(await inspectPackage(created.package_path));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page">
      <header className="page-header">
        <p className="eyebrow">SEND</p>
        <h1 ref={headingRef} tabIndex={-1}>发送交接</h1>
        <p className="page-description">选择要写入离线迁移包的本机内容。</p>
      </header>

      <section className="workflow-section" aria-labelledby="send-projects-title">
        <div className="section-title-row"><div><span className="step-number">1</span><h2 id="send-projects-title">选择项目</h2></div><span className="selection-count">已选 {projects.size}</span></div>
        <div className="choice-list">
          {projectOptions.map((project) => (
            <label className="choice-row" key={project.id}>
              <input type="checkbox" checked={projects.has(project.path)} onChange={() => toggle(setProjects, projects, project.path)} aria-label={`选择项目 ${project.name}`} />
              <span><strong>{project.name}</strong><code>{project.path}</code></span>
              <small>{project.fileCount ? `${project.fileCount} 个文件` : "已检测"}</small>
            </label>
          ))}
          {!projectOptions.length && <p className="empty-state">未检测到可发送的项目</p>}
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-content-title">
        <div className="section-title-row"><div><span className="step-number">2</span><h2 id="send-content-title">选择对话与内容</h2></div><span className="selection-count">至少选择一项</span></div>
        <div className="choice-list compact-choices">
          {conversationOptions.map((conversation) => (
            <label className="choice-row" key={conversation.task_id}>
              <input type="checkbox" checked={conversations.has(conversation.task_id)} onChange={() => toggle(setConversations, conversations, conversation.task_id)} aria-label={`选择对话 ${conversation.title}`} />
              <MessageSquareText aria-hidden="true" /><span><strong>{conversation.title}</strong><small>{conversation.updated_at}</small></span>
            </label>
          ))}
          <label className="choice-row"><input type="checkbox" checked={skills} onChange={(event) => setSkills(event.target.checked)} /><Sparkles aria-hidden="true" /><span><strong>技能</strong><small>{inventory?.counts.skills ?? 0} 项</small></span></label>
          <label className="choice-row"><input type="checkbox" checked={plugins} onChange={(event) => setPlugins(event.target.checked)} /><Puzzle aria-hidden="true" /><span><strong>插件</strong><small>{inventory?.counts.plugins ?? 0} 项</small></span></label>
          <label className="choice-row"><input type="checkbox" checked={images} onChange={(event) => setImages(event.target.checked)} /><Image aria-hidden="true" /><span><strong>生成图片</strong><small>{inventory?.counts.generated_images ?? 0} 项</small></span></label>
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-output-title">
        <div className="section-title-row"><div><span className="step-number">3</span><h2 id="send-output-title">输出位置</h2></div></div>
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>ReHome 包</strong><small>{outputPath ?? "创建时选择 .rehome 保存位置"}</small></span></div><button className="secondary-button" type="button" onClick={() => void chooseOutput()}><FolderOpen aria-hidden="true" />选择保存位置</button></div>
        <div className="command-row"><p>{!projects.size ? "请先选择项目" : !hasContent ? "请选择至少一个对话或内容类别" : "选择已完成，可以创建迁移包"}</p><button className="command-button" type="button" disabled={!canCreate} onClick={() => void handleCreate()}>{busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <PackagePlus aria-hidden="true" />}创建 ReHome 包</button></div>
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
      </section>

      {report && packagePreview && (
        <section className="result-panel" aria-labelledby="package-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="package-result-title">迁移包已创建</h2></div><span className="status status-success">校验通过</span></div>
          <div className="result-grid"><span>大小<strong>{formatBytes(report.bytes_written)}</strong></span><span>SHA-256<strong className="hash-text">{packagePreview.archive_hash}</strong></span><span>内容<strong>{report.counts.project_files} 个项目文件 / {report.counts.conversations} 个对话</strong></span></div>
          <div className="result-actions"><code>{report.package_path}</code><button className="secondary-button" type="button" onClick={() => void openPath(report.package_path)}><FolderOpen aria-hidden="true" />在文件夹中显示</button></div>
        </section>
      )}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function pathName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}
