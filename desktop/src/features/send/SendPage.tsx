import {
  useEffect,
  useMemo,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  CheckCircle2,
  Bot,
  ChevronDown,
  ChevronRight,
  FileArchive,
  FolderOpen,
  Image,
  LoaderCircle,
  MessageSquareText,
  PackagePlus,
  Puzzle,
  Sparkles,
} from "lucide-react";

import StepBar from "../../components/StepBar";
import SelectionCheckbox from "../../components/SelectionCheckbox";
import { createPackage, openPath } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import SupportPanel from "../support/SupportPanel";
import {
  errorMessage,
  supportIdFromError,
  type CodexInventory,
  type ConversationEntry,
  type CreatePackageReport,
  type OptionalContentEntry,
} from "../../lib/types";

interface SendPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
  onOperationStart: () => void;
  onOperationEnd: () => void;
  onHome: () => void;
}

export default function SendPage({
  headingRef,
  inventory,
  onOperationStart,
  onOperationEnd,
  onHome,
}: SendPageProps) {
  const { t } = useI18n();
  const [step, setStep] = useState<"selection" | "review">("selection");
  const [search, setSearch] = useState("");
  const [projects, setProjects] = useState<Set<string>>(new Set());
  const [conversations, setConversations] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [skills, setSkills] = useState<Set<string>>(new Set());
  const [plugins, setPlugins] = useState<Set<string>>(new Set());
  const [images, setImages] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<CreatePackageReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [supportId, setSupportId] = useState<string | null>(null);

  const projectGroups = useMemo(() => {
    if (!inventory) return [];
    return inventory.projects.map((project) => ({
      ...project,
      conversations: inventory.conversations.filter(
        (conversation) => conversation.project_id === project.project_id,
      ),
    }));
  }, [inventory]);
  const unassociatedConversations = useMemo(
    () =>
      inventory?.conversations.filter(
        (conversation) => conversation.project_id === null,
      ) ?? [],
    [inventory],
  );

  const hasContent =
    projects.size +
      conversations.size +
      skills.size +
      plugins.size +
      images.size >
    0;
  const hasSelectableContent = Boolean(
    inventory &&
      inventory.projects.filter((project) => project.source_available).length +
        inventory.conversations.length +
        inventory.skills.length +
        inventory.plugins.length +
        inventory.generated_images.length >
        0,
  );
  const allContentSelected = Boolean(
    inventory &&
      hasSelectableContent &&
      inventory.projects
        .filter((project) => project.source_available)
        .every((project) => projects.has(project.project_id)) &&
      inventory.conversations.every((conversation) =>
        conversations.has(conversation.task_id),
      ) &&
      inventory.skills.every((skill) => skills.has(skill.content_id)) &&
      inventory.plugins.every((plugin) => plugins.has(plugin.content_id)) &&
      inventory.generated_images.every((image) => images.has(image.content_id)),
  );
  const canCreate = Boolean(inventory && hasContent && !busy);

  function toggle(
    setter: (value: Set<string>) => void,
    current: Set<string>,
    value: string,
  ) {
    const next = new Set(current);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setter(next);
  }

  function selectRecommended(items: ConversationEntry[]) {
    const next = new Set(conversations);
    for (const item of items) next.delete(item.task_id);
    for (const item of items) {
      if (!item.classification) next.add(item.task_id);
    }
    setConversations(next);
  }

  function toggleProject(projectId: string, items: ConversationEntry[]) {
    const nextProjects = new Set(projects);
    const nextConversations = new Set(conversations);
    if (nextProjects.has(projectId)) {
      nextProjects.delete(projectId);
      for (const item of items) nextConversations.delete(item.task_id);
    } else {
      nextProjects.add(projectId);
      for (const item of items) nextConversations.add(item.task_id);
      setExpanded((current) => new Set(current).add(projectId));
    }
    setProjects(nextProjects);
    setConversations(nextConversations);
  }

  function toggleAllContent() {
    if (!inventory) return;

    if (allContentSelected) {
      setProjects(new Set());
      setConversations(new Set());
      setSkills(new Set());
      setPlugins(new Set());
      setImages(new Set());
      return;
    }

    setProjects(
      new Set(
        inventory.projects
          .filter((project) => project.source_available)
          .map((project) => project.project_id),
      ),
    );
    setConversations(
      new Set(
        inventory.conversations.map((conversation) => conversation.task_id),
      ),
    );
    setSkills(new Set(inventory.skills.map((skill) => skill.content_id)));
    setPlugins(new Set(inventory.plugins.map((plugin) => plugin.content_id)));
    setImages(
      new Set(inventory.generated_images.map((image) => image.content_id)),
    );
  }

  async function handleCreate() {
    if (!inventory || !canCreate) return;
    setError(null);
    setSupportId(null);
    setBusy(true);
    onOperationStart();
    try {
      const created = await createPackage({
        project_ids: [...projects],
        conversation_ids: [...conversations],
        skill_ids: [...skills],
        plugin_ids: [...plugins],
        generated_image_ids: [...images],
      });
      if (created) {
        setReport(created);
      }
    } catch (caught) {
      setSupportId(supportIdFromError(caught));
      const message = errorMessage(caught);
      if (message.includes("source file kept changing while being copied")) {
        setError(
          t(
            "有文件在打包过程中仍被修改。请稍等几秒后重试；如果反复出现，请先完全退出 Codex。\n{message}",
            { message },
          ),
        );
      } else if (message.includes("private staging cannot be inside")) {
        setError(
          t(
            "临时目录与所选目录重叠。请根据下方路径，缩小所选项目范围或更换迁移包保存文件夹；不要选择整个用户目录或磁盘。\n{message}",
            { message },
          ),
        );
      } else if (
        message.includes(
          "symbolic links are not allowed in selected Codex bundles",
        )
      ) {
        setError(
          t(
            "所选 Skill 或插件包含符号链接。请取消选择对应的 Skill 或插件后重试；不要删除链接指向的原文件。\n{message}",
            { message },
          ),
        );
      } else {
        setError(message);
      }
    } finally {
      setBusy(false);
      onOperationEnd();
    }
  }

  useEffect(() => {
    if (!error && !supportId) headingRef.current?.focus();
  }, [step, busy, report, error, supportId]);
  const query = search.trim().toLocaleLowerCase();
  const filteredGroups = projectGroups.filter(
    (project) =>
      !query ||
      project.name.toLocaleLowerCase().includes(query) ||
      project.source_path.toLocaleLowerCase().includes(query) ||
      project.conversations.some((chat) =>
        chat.title.toLocaleLowerCase().includes(query),
      ),
  );
  async function reveal() {
    if (!report) return;
    setError(null);
    try {
      await openPath(report.reveal_id);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }
  const summary = t(
    "{projects} 个项目 · {conversations} 个对话 · {skills} 个 Skills",
    {
      projects: projects.size,
      conversations: conversations.size,
      skills: skills.size,
    },
  );
  if (busy)
    return (
      <div className="page processing-page" aria-busy="true">
        <LoaderCircle className="processing-icon spin" aria-hidden="true" />
        <h1 ref={headingRef} tabIndex={-1}>
          {t("正在创建迁移包")}
        </h1>
        <p role="status">
          {t("正在创建迁移包。内容较多时可能需要几分钟，请保持 ReHome 打开。")}
        </p>
        <p className="muted">{summary}</p>
      </div>
    );
  if (report)
    return (
      <div className="page">
        <StepBar
          label={t("导出 · 3 / 3")}
          onBack={() => {
            setReport(null);
            setError(null);
            setSupportId(null);
            setStep("review");
          }}
        />
        <header className="page-header">
          <CheckCircle2 className="result-symbol" aria-hidden="true" />
          <h1 ref={headingRef} tabIndex={-1}>
            {t("迁移包已创建")}
          </h1>
          <p className="page-description">
            {t("把这个文件带到新电脑，再用 ReHome 导入。")}
          </p>
        </header>
        <div className="package-file">
          <FileArchive aria-hidden="true" />
          <div>
            <strong>{report.package_path.split(/[\\/]/).pop()}</strong>
            <small>
              {formatBytes(report.bytes_written)} ·{" "}
              {t("{count} 个项目", { count: report.counts.projects })} ·{" "}
              {t("{count} 个对话", { count: report.counts.conversations })}
            </small>
          </div>
        </div>
        <p className="privacy-note">
          {t("请保留迁移包，直到新电脑的旧对话确认能用。")}
        </p>
        <details className="technical-details">
          <summary>{t("查看保存位置与校验")}</summary>
          <code>{report.package_path}</code>
          <code>SHA-256: {report.archive_hash}</code>
        </details>
        {error && (
          <p className="inline-state status-error" role="alert">
            {error}
          </p>
        )}
        <footer className="action-footer">
          <button className="secondary-button" onClick={onHome}>
            {t("回首页")}
          </button>
          <button className="command-button" onClick={() => void reveal()}>
            <FolderOpen aria-hidden="true" />
            {t("在文件夹中显示")}
          </button>
        </footer>
      </div>
    );
  return (
    <div className="page send-page">
      <StepBar
        label={t(step === "selection" ? "导出 · 1 / 3" : "导出 · 2 / 3")}
        onBack={() => (step === "review" ? setStep("selection") : onHome())}
      />
      <header className="page-header">
        <h1 ref={headingRef} tabIndex={-1}>
          {t(step === "selection" ? "带走哪些内容？" : "确认要带走的内容")}
        </h1>
        <p className="page-description">
          {t(
            step === "selection"
              ? "选好项目和对话，其他内容按需带走。"
              : "确认后，选择迁移包的保存位置。",
          )}
        </p>
      </header>
      {step === "selection" ? (
        <>
          <div className="selection-toolbar">
            <label className="search-field">
              <input
                type="search"
                aria-label={t("搜索项目或对话")}
                placeholder={t("搜索项目或对话")}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
            <label className="global-select-toggle">
              <SelectionCheckbox
                checked={allContentSelected}
                mixed={hasContent && !allContentSelected}
                onChange={toggleAllContent}
                disabled={!hasSelectableContent}
                aria-label={t("全选迁移内容")}
              />
              <span>{t("全选迁移内容")}</span>
            </label>
          </div>
          <section
            className="workflow-section"
            aria-labelledby="send-projects-title"
          >
            <h2 id="send-projects-title" className="list-heading">
              {t("选择项目与对话")}
            </h2>
            <div className="project-list">
              {filteredGroups.map((project) => (
                <ProjectChoice
                  key={project.project_id}
                  name={project.name}
                  path={formatDisplayPath(project.source_path)}
                  fileCount={project.file_count}
                  sourceAvailable={project.source_available}
                  conversations={project.conversations}
                  projectSelected={projects.has(project.project_id)}
                  expanded={expanded.has(project.project_id) || Boolean(query)}
                  selectedConversations={conversations}
                  onToggleProject={() =>
                    toggleProject(project.project_id, project.conversations)
                  }
                  onToggleExpanded={() =>
                    toggle(setExpanded, expanded, project.project_id)
                  }
                  onToggleConversation={(id) =>
                    toggle(setConversations, conversations, id)
                  }
                  onSelectRecommended={() =>
                    selectRecommended(project.conversations)
                  }
                />
              ))}
              {!filteredGroups.length && (
                <p className="empty-state">
                  {t(
                    query
                      ? "没有匹配的项目或对话"
                      : "未检测到 Codex 已登记的本机项目",
                  )}
                </p>
              )}
              {unassociatedConversations.some(
                (chat) =>
                  !query || chat.title.toLocaleLowerCase().includes(query),
              ) && (
                <ProjectChoice
                  name={t("未归属项目的对话")}
                  path={t("只迁移对话，不包含项目文件")}
                  fileCount={null}
                  conversations={unassociatedConversations.filter(
                    (chat) =>
                      !query || chat.title.toLocaleLowerCase().includes(query),
                  )}
                  projectSelected={false}
                  expanded={expanded.has("unassociated") || Boolean(query)}
                  selectedConversations={conversations}
                  onToggleExpanded={() =>
                    toggle(setExpanded, expanded, "unassociated")
                  }
                  onToggleConversation={(id) =>
                    toggle(setConversations, conversations, id)
                  }
                  onSelectRecommended={() =>
                    selectRecommended(unassociatedConversations)
                  }
                />
              )}
            </div>
          </section>
          <div className="optional-content-list">
            <OptionalContentGroup
              id="skills"
              title="Skills"
              description={t("迁移你希望在新电脑继续使用的能力")}
              icon={<Sparkles aria-hidden="true" />}
              items={inventory?.skills ?? []}
              selected={skills}
              expanded={expanded.has("skills")}
              onToggleExpanded={() => toggle(setExpanded, expanded, "skills")}
              onChange={setSkills}
            />
            <details className="more-content">
              <summary>
                {t("更多内容")} · Plugins / {t("生成图片")}
              </summary>
              <OptionalContentGroup
                id="plugins"
                title="Plugins"
                description={t("通常可以在新电脑重装，也可以选择带走")}
                icon={<Puzzle aria-hidden="true" />}
                items={inventory?.plugins ?? []}
                selected={plugins}
                expanded={expanded.has("plugins")}
                onToggleExpanded={() =>
                  toggle(setExpanded, expanded, "plugins")
                }
                onChange={setPlugins}
              />
              <OptionalContentGroup
                id="images"
                title={t("生成图片")}
                description={t("只在需要保留历史生成物时选择")}
                icon={<Image aria-hidden="true" />}
                items={inventory?.generated_images ?? []}
                selected={images}
                expanded={expanded.has("images")}
                onToggleExpanded={() => toggle(setExpanded, expanded, "images")}
                onChange={setImages}
              />
            </details>
          </div>
          {inventory && inventory.warnings.length > 0 && (
            <details className="technical-details">
              <summary>{t("查看检测提示")}</summary>
              {inventory.warnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </details>
          )}
        </>
      ) : (
        <section className="export-review">
          {projectGroups
            .filter(
              (project) =>
                projects.has(project.project_id) ||
                project.conversations.some((chat) =>
                  conversations.has(chat.task_id),
                ),
            )
            .map((project) => (
              <div className="summary-row" key={project.project_id}>
                <strong>{project.name}</strong>
                <span>
                  {t("{count} 个对话", {
                    count: project.conversations.filter((chat) =>
                      conversations.has(chat.task_id),
                    ).length,
                  })}{" "}
                  ·{" "}
                  {t(
                    projects.has(project.project_id)
                      ? "包含项目文件"
                      : "仅对话",
                  )}
                </span>
              </div>
            ))}
          {unassociatedConversations.some((chat) =>
            conversations.has(chat.task_id),
          ) && (
            <div className="summary-row">
              <strong>{t("未归属项目的对话")}</strong>
              <span>
                {t("{count} 个对话", {
                  count: unassociatedConversations.filter((chat) =>
                    conversations.has(chat.task_id),
                  ).length,
                })}
              </span>
            </div>
          )}
          <div className="summary-row">
            <strong>Skills / Plugins / {t("生成图片")}</strong>
            <span>
              {skills.size} / {plugins.size} / {images.size}
            </span>
          </div>
          <p className="privacy-note">{t("登录信息和敏感文件不会打包。")}</p>
          <p className="muted">{t("体积将在创建时确定。")}</p>
          <details className="technical-details">
            <summary>{t("如何分批迁移？")}</summary>
            <p>{t("每次只选一部分对话，分别保存成不同的迁移包。同一项目分批迁移时，每批都保留项目勾选，并在新电脑导入到同一个项目保存位置。未变化的项目文件会跳过；有变化的文件仍需确认冲突。")}</p>
          </details>
          <details className="technical-details">
            <summary>{t("查看排除内容")}</summary>
            <p>
              {t(
                "登录凭据、密钥、.env、Git 历史、依赖目录和运行中的进程不会迁移。新电脑仍需安装环境并登录 Codex。",
              )}
            </p>
          </details>
        </section>
      )}
      {error && (
        <p className="inline-state status-error" role="alert">
          {error}
        </p>
      )}
      {supportId && (
        <SupportPanel
          key={supportId}
          source={{ kind: "incident", support_id: supportId }}
        />
      )}
      <footer className="action-footer">
        <div>
          <strong>{summary}</strong>
          <small>
            {t("Plugins {plugins} · 生成图片 {images}", {
              plugins: plugins.size,
              images: images.size,
            })}
          </small>
        </div>
        {step === "selection" ? (
          <button
            className="command-button"
            disabled={!canCreate}
            onClick={() => setStep("review")}
          >
            {t("继续")}
          </button>
        ) : (
          <div className="footer-buttons">
            <button
              className="secondary-button"
              onClick={() => setStep("selection")}
            >
              {t("修改选择")}
            </button>
            <button
              className="command-button"
              disabled={!canCreate}
              onClick={() => void handleCreate()}
            >
              <PackagePlus aria-hidden="true" />
              {t("选择保存位置并创建")}
            </button>
          </div>
        )}
      </footer>
    </div>
  );
}

interface ProjectChoiceProps {
  name: string;
  path: string;
  fileCount: number | null;
  sourceAvailable?: boolean;
  conversations: ConversationEntry[];
  projectSelected: boolean;
  expanded: boolean;
  selectedConversations: Set<string>;
  onToggleProject?: () => void;
  onToggleExpanded: () => void;
  onToggleConversation: (id: string) => void;
  onSelectRecommended: () => void;
}

function ProjectChoice({
  name,
  path,
  fileCount,
  sourceAvailable = true,
  conversations,
  projectSelected,
  expanded,
  selectedConversations,
  onToggleProject,
  onToggleExpanded,
  onToggleConversation,
  onSelectRecommended,
}: ProjectChoiceProps) {
  const { locale, t } = useI18n();
  const subagents = conversations.filter(
    (conversation) => conversation.classification,
  ).length;
  const mainConversations = conversations.length - subagents;
  return (
    <div
      className={`project-choice${sourceAvailable ? "" : " project-choice-missing"}`}
    >
      <div className="project-choice-header">
        {onToggleProject ? (
          <label className="project-file-toggle">
            <SelectionCheckbox
              mixed={
                (projectSelected ||
                  conversations.some((item) =>
                    selectedConversations.has(item.task_id),
                  )) &&
                (!projectSelected ||
                  !conversations.every((item) =>
                    selectedConversations.has(item.task_id),
                  ))
              }
              checked={projectSelected}
              onChange={onToggleProject}
              disabled={!sourceAvailable}
              aria-label={t("选择项目 {name}", { name })}
            />
            <span className="project-copy">
              <strong>{name}</strong>
              <code>{path}</code>
              {!sourceAvailable && (
                <small>{t("项目文件夹已不存在，仅可迁移下面的对话")}</small>
              )}
            </span>
          </label>
        ) : (
          <span className="project-copy project-copy-unassociated">
            <strong>{name}</strong>
            <small>{path}</small>
          </span>
        )}
        <button
          className="project-expand"
          type="button"
          aria-expanded={expanded}
          aria-label={t(expanded ? "收起项目 {name}" : "展开项目 {name}", {
            name,
          })}
          onClick={onToggleExpanded}
        >
          <span>
            {t("{count} 个对话", { count: conversations.length })}
            {fileCount !== null &&
              (sourceAvailable
                ? ` · ${fileCount ? t("{count} 个文件", { count: fileCount }) : t("项目文件将在打包时统计")}`
                : ` · ${t("项目文件缺失")}`)}
          </span>
          {expanded ? (
            <ChevronDown aria-hidden="true" />
          ) : (
            <ChevronRight aria-hidden="true" />
          )}
        </button>
      </div>
      {expanded && (
        <div
          className="project-conversations"
          aria-label={t("{name} 的对话", { name })}
        >
          {conversations.length > 0 && (
            <div className="conversation-toolbar">
              <span>
                {t(
                  "主对话 {main} · 子 Agent {subagents} · 单独勾对话不含项目文件",
                  { main: mainConversations, subagents },
                )}
              </span>
              {mainConversations > 0 && (
                <button type="button" onClick={onSelectRecommended}>
                  {t("只选主对话")}
                </button>
              )}
            </div>
          )}
          {conversations.map((conversation) => (
            <label className="conversation-choice" key={conversation.task_id}>
              <input
                type="checkbox"
                checked={selectedConversations.has(conversation.task_id)}
                onChange={() => onToggleConversation(conversation.task_id)}
                aria-label={t("选择对话 {name}", { name: conversation.title })}
              />
              {conversation.classification ? (
                <Bot aria-hidden="true" />
              ) : (
                <MessageSquareText aria-hidden="true" />
              )}
              <span>
                <strong>{conversation.title}</strong>
                <small className="conversation-details">
                  <span
                    className={
                      conversation.classification
                        ? "conversation-badge badge-subagent"
                        : "conversation-badge badge-main"
                    }
                  >
                    {conversation.classification
                      ? `${t("子 Agent")}${conversation.classification.depth ? ` · L${conversation.classification.depth}` : ""}`
                      : t("主对话")}
                  </span>
                  <span>
                    {t(
                      conversation.classification
                        ? "辅助记录，通常可不迁移"
                        : "建议迁移",
                    )}
                  </span>
                  <time>{formatDate(conversation.updated_at, locale)}</time>
                </small>
              </span>
            </label>
          ))}
          {!conversations.length && (
            <p className="project-empty">{t("这个项目下暂无可迁移对话")}</p>
          )}
        </div>
      )}
    </div>
  );
}

interface OptionalContentGroupProps {
  id: string;
  title: string;
  description: string;
  icon: ReactNode;
  items: OptionalContentEntry[];
  selected: Set<string>;
  expanded: boolean;
  onToggleExpanded: () => void;
  onChange: (value: Set<string>) => void;
}

function OptionalContentGroup({
  id,
  title,
  description,
  icon,
  items,
  selected,
  expanded,
  onToggleExpanded,
  onChange,
}: OptionalContentGroupProps) {
  const { t } = useI18n();
  const allSelected =
    items.length > 0 && items.every((item) => selected.has(item.content_id));
  function toggleItem(contentId: string) {
    const next = new Set(selected);
    if (next.has(contentId)) next.delete(contentId);
    else next.add(contentId);
    onChange(next);
  }
  function toggleAll() {
    onChange(
      allSelected ? new Set() : new Set(items.map((item) => item.content_id)),
    );
  }

  return (
    <div className="optional-content-group">
      <div className="optional-content-header">
        <label className="optional-all-toggle">
          <SelectionCheckbox
            mixed={selected.size > 0 && !allSelected}
            checked={allSelected}
            onChange={toggleAll}
            disabled={!items.length}
            aria-label={t("全选 {name}", { name: title })}
          />
          {icon}
          <span>
            <strong>{title}</strong>
            <small>{description}</small>
          </span>
        </label>
        <button
          type="button"
          className="project-expand"
          aria-expanded={expanded}
          aria-controls={`optional-${id}`}
          onClick={onToggleExpanded}
        >
          <span>
            {t("已选 {selected} / {total}", {
              selected: selected.size,
              total: items.length,
            })}
          </span>
          {expanded ? (
            <ChevronDown aria-hidden="true" />
          ) : (
            <ChevronRight aria-hidden="true" />
          )}
        </button>
      </div>
      {expanded && (
        <div className="optional-items" id={`optional-${id}`}>
          {items.map((item) => (
            <div
              className={`optional-item${item.thumbnail_data_url ? " optional-item-image" : ""}`}
              key={item.content_id}
            >
              <input
                type="checkbox"
                checked={selected.has(item.content_id)}
                onChange={() => toggleItem(item.content_id)}
                aria-label={t("选择 {name}", { name: item.name })}
              />
              {item.thumbnail_data_url && (
                <img
                  className="image-thumbnail"
                  src={item.thumbnail_data_url}
                  alt=""
                />
              )}
              <span>
                <strong>{item.name}</strong>
                <small>{item.relative_path}</small>
              </span>
              <small className="item-size">
                {formatBytes(item.size_bytes)}
              </small>
              {item.reveal_id && (
                <button
                  className="icon-button image-reveal-button"
                  type="button"
                  title={t("在文件夹中显示")}
                  aria-label={t("在文件夹中显示 {name}", { name: item.name })}
                  onClick={() => void openPath(item.reveal_id!)}
                >
                  <FolderOpen aria-hidden="true" />
                </button>
              )}
            </div>
          ))}
          {!items.length && (
            <p className="project-empty">{t("没有检测到这类内容")}</p>
          )}
        </div>
      )}
    </div>
  );
}

function formatDisplayPath(value: string): string {
  return value.startsWith("\\\\?\\") ? value.slice(4) : value;
}

function formatDate(value: string, locale: "zh-CN" | "en"): string {
  if (!value) return locale === "en" ? "Time unknown" : "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : date.toLocaleString(locale, { hour12: false });
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
