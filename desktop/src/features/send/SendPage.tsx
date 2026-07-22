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

import { createPackage, openPath } from "../../lib/api";
import { errorMessage, type CodexInventory, type CreatePackageReport } from "../../lib/types";

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
  const [report, setReport] = useState<CreatePackageReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hasContent = conversations.size > 0 || skills || plugins || images;
  const canCreate = Boolean(inventory && projects.size > 0 && hasContent && !busy);
  const projectOptions = useMemo(() => {
    if (!inventory) return [];
    return inventory.projects.map((project) => ({
      id: project.project_id,
      name: project.name,
      path: project.source_path,
      fileCount: project.file_count,
    }));
  }, [inventory]);
  const conversationOptions = useMemo(() => {
    if (!inventory) return [];
    return inventory.conversations.filter(
      (conversation) =>
        conversation.project_id === null || projects.has(conversation.project_id),
    );
  }, [inventory, projects]);

  function toggle(setter: (value: Set<string>) => void, current: Set<string>, value: string) {
    const next = new Set(current);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setter(next);
  }

  function toggleProject(projectId: string) {
    const next = new Set(projects);
    if (next.has(projectId)) next.delete(projectId);
    else next.add(projectId);
    setProjects(next);
    if (inventory) {
      const allowed = new Set(
        inventory.conversations
          .filter(
            (conversation) =>
              conversation.project_id === null ||
              (conversation.project_id !== null && next.has(conversation.project_id)),
          )
          .map((conversation) => conversation.task_id),
      );
      setConversations(
        (current) => new Set([...current].filter((conversationId) => allowed.has(conversationId))),
      );
    }
  }

  async function handleCreate() {
    if (!inventory || !canCreate) return;
    setError(null);
    setBusy(true);
    try {
      const created = await createPackage({
        project_ids: [...projects],
        conversation_ids: [...conversations],
        include_skills: skills,
        include_plugins: plugins,
        include_generated_images: images,
      });
      if (!created) return;
      setReport(created);
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
              <input type="checkbox" checked={projects.has(project.id)} onChange={() => toggleProject(project.id)} aria-label={`选择项目 ${project.name}`} />
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
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>ReHome 包</strong><small>创建时通过系统窗口选择 .rehome 保存位置</small></span></div></div>
        <div className="command-row"><p>{!projects.size ? "请先选择项目" : !hasContent ? "请选择至少一个对话或内容类别" : "选择已完成，可以创建迁移包"}</p><button className="command-button" type="button" disabled={!canCreate} onClick={() => void handleCreate()}>{busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <PackagePlus aria-hidden="true" />}创建 ReHome 包</button></div>
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
      </section>

      {report && (
        <section className="result-panel" aria-labelledby="package-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="package-result-title">迁移包已创建</h2></div><span className="status status-success">校验通过</span></div>
          <div className="result-grid"><span>大小<strong>{formatBytes(report.bytes_written)}</strong></span><span>SHA-256<strong className="hash-text">{report.archive_hash}</strong></span><span>内容<strong>{report.counts.project_files} 个项目文件 / {report.counts.conversations} 个对话</strong></span></div>
          <div className="result-actions"><code>{report.package_path}</code><button className="secondary-button" type="button" onClick={() => void openPath(report.reveal_id)}><FolderOpen aria-hidden="true" />在文件夹中显示</button></div>
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
