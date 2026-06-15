import React from "react";
import ReactDOM from "react-dom/client";
import ReactMarkdown from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import remarkGfm from "remark-gfm";
import {
  Bot,
  Brain,
  CheckCircle2,
  CircleAlert,
  Clipboard,
  FolderGit2,
  GitPullRequest,
  History,
  Inbox,
  MessageSquareText,
  Plug,
  Radar,
  RefreshCw,
  Search,
  TerminalSquare,
  Wrench,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import "./styles.css";

type ContextFileObservation = { path: string; present: boolean };

type InboxSummary = {
  installed: boolean;
  docs_present: boolean;
  schema_present: boolean;
  feedback_present: boolean;
  counts: Record<string, number>;
  active_count: number;
  latest_title: string | null;
  latest_body: string | null;
};

type GitHubIssueSummary = {
  repo: string | null;
  available: boolean;
  open_count: number;
  error: string | null;
};

type GitHubRepoRecord = {
  project_name: string;
  project_path: string;
  repo: string;
  owner_avatar_url: string | null;
  repo_image_url: string | null;
  local_icon_path: string | null;
  local_icon_url: string | null;
  description: string | null;
  homepage_url: string | null;
  url: string | null;
  topics: string[];
  stars: number | null;
  forks: number | null;
  license: string | null;
  open_prs: number | null;
  default_branch: string | null;
  pushed_at: string | null;
};

type GitHubIssueRecord = {
  project_name: string;
  project_path: string;
  repo: string;
  number: number;
  title: string;
  body: string;
  labels: string[];
  url: string | null;
  updated_at: string | null;
};

type GitHubFreshness = {
  fetched_at: number; // unix epoch seconds
  stale: boolean;
  source: string; // "github-cache" | "github-live"
  error: string | null;
};

type GitHubRepoResponse = {
  record: GitHubRepoRecord | null;
  freshness: GitHubFreshness;
};

type GitHubIssuesResponse = {
  records: GitHubIssueRecord[];
  freshness: GitHubFreshness;
};

type LocalObservationEvent = {
  reason: string;
  observed_at: number;
  paths: string[];
};

function fmtEpoch(epochSecs: number): string {
  if (epochSecs === 0) return "never";
  const d = new Date(epochSecs * 1000);
  const now = Date.now();
  const diff = Math.floor((now - epochSecs * 1000) / 1000);
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return d.toLocaleDateString();
}

type AgentInboxRecord = {
  project_name: string;
  project_path: string;
  id: string;
  kind: string;
  status: string;
  title: string | null;
  body: string;
  plan: string | null;
  route: string | null;
  created_at: string | null;
  updated_at: string | null;
  context: unknown | null;
  comments: unknown | null;
  agent_notes: unknown | null;
};

type ProjectVisuals = {
  org_avatar_url: string | null;
  repo_image_url: string | null;
  local_icon_path: string | null;
  local_icon_url: string | null;
};

type ProjectObservation = {
  name: string;
  path: string;
  context_files: ContextFileObservation[];
  inbox: InboxSummary;
  github_issues: GitHubIssueSummary;
  visuals: ProjectVisuals;
  readme: string | null;
  latest_commit_epoch: number | null;
  latest_commit: string | null;
};

type GitSummary = {
  branch: string | null;
  dirty_count: number;
  ahead: number;
  behind: number;
  last_commit: string | null;
};

type LibraryAsset = {
  kind: string;
  name: string;
  path: string;
  description: string | null;
  tags: string[];
  body: string;
};

type AgentLibraryOverview = {
  root: string;
  prompts: LibraryAsset[];
  recipes: LibraryAsset[];
  skills: LibraryAsset[];
};

type AgentMemoryFile = {
  agent: string;
  name: string;
  path: string;
  content: string;
};

type ProjectSkillRecord = {
  name: string;
  scope: string;
  source: string;
  path: string;
  seen_by: string[];
  description: string | null;
};

type McpServerRecord = {
  name: string;
  command: string;
  args: string[];
};

type AgentSessionRecord = {
  agent: string;
  id: string | null;
  started_at: string;
  first_message: string | null;
  path: string | null;
};

type ProjectAgentsOverview = {
  memories: AgentMemoryFile[];
  skills: ProjectSkillRecord[];
  mcp_servers: McpServerRecord[];
  sessions: AgentSessionRecord[];
};

type AppOverview = {
  contract: string;
  projects_root: string;
  projects: ProjectObservation[];
  inbox_records: AgentInboxRecord[];
  github_issue_records: GitHubIssueRecord[];
  agent_library: AgentLibraryOverview;
};

const ACTIVE_STATUSES = ["new", "planned", "accepted", "in_progress"];
const ALL_STATUSES = [...ACTIVE_STATUSES, "done", "wontfix"];

type DashboardTab = "overview" | "agent-inbox" | "github";
type ProjectTab = "overview" | "agent-inbox" | "context" | "memories" | "agents" | "github";

function recordTitle(record: AgentInboxRecord) {
  return record.title || record.body.slice(0, 72) || record.id;
}

function buildPlanPrompt(record: AgentInboxRecord) {
  return `Read .agent/inbox/README.md in ${record.project_path}, then plan inbox record ${record.id}.

Project: ${record.project_name}
Kind: ${record.kind}
Status: ${record.status}
Route: ${record.route ?? "unknown"}

User request:
${record.body}

Workflow rule: plan -> write -> then respond. Update the record with status planned, a concrete plan, updatedAt, and an agentNotes entry before describing the plan in chat.`;
}

function buildImplementPrompt(record: AgentInboxRecord) {
  return `Read .agent/inbox/README.md in ${record.project_path}, then implement accepted inbox record ${record.id}.

Project: ${record.project_name}
Kind: ${record.kind}
Status: ${record.status}

User request:
${record.body}

Saved plan:
${record.plan ?? "No saved plan present. Stop and report that the record needs a plan before implementation."}

Follow the inbox lifecycle: mark in_progress, implement, validate, then mark done with an agentNotes summary.`;
}

function buildMissingSpecPrompt(project: ProjectObservation) {
  return `Draft a SPEC.md product contract for this project.

Project path: ${project.path}

Read the README, agent context files, docs, package manifests, and source layout. Produce a SPEC.md that captures product thesis, non-negotiable constraints, shipped behavior, planned behavior, explicit non-goals, and validation criteria. Do not invent roadmap items without repo evidence.`;
}

function buildMissingAgentsPrompt(project: ProjectObservation) {
  return `Draft an AGENTS.md context file for this project.

Project path: ${project.path}

Read README, SPEC.md if present, docs, package manifests, scripts, and source layout. Capture what the project is, stack, run/validation commands, repo layout, product rules, and gotchas future coding agents must know.`;
}

function buildAgentInboxPlanAllPrompt(project: ProjectObservation) {
  return `Read .agent/inbox/README.md in ${project.path}, then plan all agent inbox records with status new.

Follow the workflow rule: plan -> write -> then respond. For each new record, write a concrete plan into the record, set status to planned, update updatedAt, and append an agentNotes entry. Do not implement until accepted.`;
}

function buildAcceptedInboxPrompt(project: ProjectObservation) {
  return `Read .agent/inbox/README.md in ${project.path}, then implement all accepted agent inbox records.

For each accepted record: read the saved plan, mark in_progress, implement only the accepted scope, validate, then mark done with an agentNotes summary. Stop if a record has no plan or the plan is stale.`;
}

function buildIssueTriagePrompt(project: ProjectObservation) {
  return `Triage open GitHub issues for this project.

Project path: ${project.path}
Repo: ${project.github_issues.repo ?? "unknown"}

Read project context and open issue summaries. Group related issues, identify high-leverage next actions, and propose implementation plans. Do not mutate GitHub; report recommended labels/issues/actions only.`;
}

function buildIssuePrompt(issue: GitHubIssueRecord) {
  return `Inspect GitHub issue #${issue.number} in ${issue.repo} for project ${issue.project_name}.

Project path: ${issue.project_path}
Issue: #${issue.number} ${issue.title}
URL: ${issue.url ?? "unknown"}
Labels: ${issue.labels.join(", ") || "none"}

Issue body:
${issue.body || "(no body)"}

Read the project context and relevant source files, then propose a concrete plan with validation steps. Do not mutate GitHub from project-index context; report the plan and suggested next action.`;
}

function countPresent(project: ProjectObservation) {
  return project.context_files.filter((item) => item.present).length;
}

function RepoAvatar({ project, repoData, size = "md" }: { project: ProjectObservation; repoData?: GitHubRepoRecord | null; size?: "sm" | "md" | "lg" }) {
  // Repo image is intentionally local app/web identity only: app icon, favicon,
  // apple-touch-icon, or logo. Do not fall back to GitHub org avatars here.
  const src = repoData?.local_icon_url ?? project.visuals.local_icon_url;
  const className = cn(
    "shrink-0 rounded-md border bg-muted object-cover",
    size === "sm" && "size-8",
    size === "md" && "size-10",
    size === "lg" && "size-16"
  );
  if (!src) return <div className={cn(className, "grid place-items-center")}><FolderGit2 className="size-4 text-muted-foreground" /></div>;
  return <img src={src} alt="" className={className} />;
}

function OrgAvatar({ project, repoData, size = "sm" }: { project: ProjectObservation; repoData?: GitHubRepoRecord | null; size?: "sm" | "md" }) {
  const src = repoData?.owner_avatar_url ?? project.visuals.org_avatar_url;
  const className = cn("shrink-0 rounded-full border bg-muted object-cover", size === "sm" ? "size-6" : "size-8");
  return src ? <img src={src} alt="" className={className} /> : null;
}

function ShellCard({ title, description, icon, children, className }: React.PropsWithChildren<{ title: string; description?: string; icon?: React.ReactNode; className?: string }>) {
  return (
    <Card className={cn("min-w-0", className)}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          {icon}
          {title}
        </CardTitle>
        {description ? <CardDescription>{description}</CardDescription> : null}
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

function StatCard({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <Card className="py-4">
      <CardContent className="px-4">
        <div className="text-2xl font-semibold tracking-tight">{value}</div>
        <div className="text-xs text-muted-foreground">{label}</div>
      </CardContent>
    </Card>
  );
}

function GroupAvatar({ group }: { group: string }) {
  const owner = group.startsWith("_") ? group.slice(1) : group;
  if (!owner || owner === "local" || owner === "Projects") return <FolderGit2 className="size-4" />;
  return <img src={`https://github.com/${owner}.png?size=64`} alt="" className="size-5 rounded-full border bg-muted object-cover" />;
}

function ProjectCard({ project, issueCount, selected, onSelect }: { project: ProjectObservation; issueCount: number; selected: boolean; onSelect: () => void }) {
  const inboxVariant = project.inbox.active_count > 0 ? "default" : project.inbox.installed ? "secondary" : "outline";
  return (
    <SidebarMenuItem>
      <SidebarMenuButton isActive={selected} size="lg" className="h-auto py-2" onClick={onSelect} tooltip={project.name}>
        <RepoAvatar project={project} size="sm" />
        <div className="min-w-0 flex-1 space-y-1">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate font-medium">{project.name}</span>
            <span className="flex shrink-0 items-center gap-1">
              <Badge variant={inboxVariant}><Inbox />{project.inbox.active_count}</Badge>
              <Badge variant="outline"><GitPullRequest />{issueCount}</Badge>
            </span>
          </div>
          <div className="truncate text-xs font-normal text-muted-foreground">{project.path}</div>
          <div className="flex justify-between gap-2 text-xs font-normal text-muted-foreground">
            <span>{countPresent(project)}/{project.context_files.length} context</span>
            <span className="truncate">{project.github_issues.repo ?? "no github"}</span>
          </div>
        </div>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

function DashboardProjectGrid({ projects, issueCountsByPath, onSelect }: { projects: ProjectObservation[]; issueCountsByPath: Map<string, number>; onSelect: (project: ProjectObservation) => void }) {
  const sorted = [...projects].sort((a, b) => (b.latest_commit_epoch ?? 0) - (a.latest_commit_epoch ?? 0) || a.name.localeCompare(b.name));
  return (
    <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
      {sorted.map((project) => {
        const issueCount = issueCountsByPath.get(project.path) ?? 0;
        return (
          <Card key={project.path} className="cursor-pointer transition-colors hover:bg-muted/30" onClick={() => onSelect(project)}>
            <CardHeader className="pb-3">
              <div className="flex items-start gap-3">
                <RepoAvatar project={project} size="md" />
                <div className="min-w-0 flex-1">
                  <CardTitle className="truncate text-base">{project.name}</CardTitle>
                  <CardDescription className="truncate">{project.path}</CardDescription>
                </div>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="grid grid-cols-3 gap-2 text-center">
                <div className="rounded-md border bg-background p-2"><div className="font-semibold">{countPresent(project)}/{project.context_files.length}</div><div className="text-xs text-muted-foreground">context</div></div>
                <div className="rounded-md border bg-background p-2"><div className="font-semibold">{project.inbox.active_count}</div><div className="text-xs text-muted-foreground">inbox</div></div>
                <div className="rounded-md border bg-background p-2"><div className="font-semibold">{issueCount}</div><div className="text-xs text-muted-foreground">issues</div></div>
              </div>
              <div className="space-y-1 text-sm">
                <div className="text-xs uppercase tracking-wide text-muted-foreground">Latest commit</div>
                <div className="line-clamp-2 min-h-10">{project.latest_commit ?? "No commit observed"}</div>
              </div>
              <div className="flex flex-wrap gap-2">
                <Badge variant={project.github_issues.repo ? "secondary" : "outline"}>{project.github_issues.repo ?? "no github"}</Badge>
                {project.inbox.installed ? <Badge variant="outline">agent inbox</Badge> : null}
              </div>
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}

function ContextPanel({ project }: { project: ProjectObservation }) {
  return (
    <ShellCard title="Context health" description="Read-only project context observations" icon={<Radar className="size-4" />}>
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        {project.context_files.map((item) => (
          <div key={item.path} className="flex min-w-0 items-center gap-2 rounded-md border bg-muted/30 px-3 py-2 text-sm">
            {item.present ? <CheckCircle2 className="size-4 text-chart-3" /> : <CircleAlert className="size-4 text-chart-4" />}
            <span className="truncate">{item.path}</span>
          </div>
        ))}
      </div>
    </ShellCard>
  );
}

function MemoriesPanel({ agents }: { agents: ProjectAgentsOverview | null }) {
  const [selectedPath, setSelectedPath] = React.useState<string | null>(null);
  const memories = agents?.memories ?? [];
  const selected = memories.find((memory) => memory.path === selectedPath) ?? memories[0] ?? null;

  React.useEffect(() => {
    if (!selectedPath && memories[0]) setSelectedPath(memories[0].path);
  }, [memories, selectedPath]);

  return (
    <div className="grid gap-4 lg:grid-cols-[320px_1fr]">
      <ShellCard title="Memories" description="Native agent memory/config surfaces" icon={<Brain className="size-4" />}>
        {memories.length === 0 ? <EmptyState>No native agent memories observed for this project.</EmptyState> : (
          <div className="space-y-2">
            {memories.map((memory) => (
              <button key={memory.path} type="button" onClick={() => setSelectedPath(memory.path)} className={cn("w-full rounded-md border p-3 text-left text-sm transition-colors hover:bg-muted/50", selected?.path === memory.path && "border-primary bg-muted") }>
                <div className="flex items-center justify-between gap-2">
                  <span className="font-medium">{memory.name}</span>
                  <Badge variant="outline">{memory.agent}</Badge>
                </div>
                <div className="mt-1 truncate text-xs text-muted-foreground">{memory.path}</div>
              </button>
            ))}
          </div>
        )}
      </ShellCard>
      <ShellCard title={selected ? `${selected.agent}: ${selected.name}` : "Memory preview"} description={selected?.path} icon={<MessageSquareText className="size-4" />}>
        {selected ? <MarkdownBlock className="max-h-[680px] overflow-auto pr-2">{selected.content}</MarkdownBlock> : <EmptyState>Select a memory file to preview it.</EmptyState>}
      </ShellCard>
    </div>
  );
}

function AgentsPanel({ agents }: { agents: ProjectAgentsOverview | null }) {
  const skills = agents?.skills ?? [];
  const mcpServers = agents?.mcp_servers ?? [];
  const sessions = agents?.sessions ?? [];
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <ShellCard title="Agent surface summary" description="Read-only native agent inventory" icon={<Bot className="size-4" />} className="lg:col-span-2">
        <div className="grid gap-2 sm:grid-cols-3">
          <StatCard label="Project skills" value={skills.length} />
          <StatCard label="MCP servers" value={mcpServers.length} />
          <StatCard label="Sessions" value={sessions.length} />
        </div>
      </ShellCard>
      <ShellCard title="Skills" description="Project-local skill directories" icon={<Wrench className="size-4" />}>
        {skills.length === 0 ? <EmptyState>No project-local skills observed.</EmptyState> : (
          <div className="space-y-2">
            {skills.map((skill) => (
              <div key={skill.path} className="rounded-md border bg-muted/30 p-3 text-sm">
                <div className="flex items-center justify-between gap-2">
                  <div className="font-medium">{skill.name}</div>
                  <Badge variant="outline">{skill.scope}</Badge>
                </div>
                {skill.description ? <p className="mt-1 text-sm text-muted-foreground">{skill.description}</p> : null}
                <div className="mt-2 flex flex-wrap gap-1">
                  <Badge variant="secondary">{skill.source}</Badge>
                  {skill.seen_by.map((agent) => <Badge key={agent} variant="outline">{agent}</Badge>)}
                </div>
                <code className="mt-2 block truncate text-xs text-muted-foreground">{skill.path}</code>
              </div>
            ))}
          </div>
        )}
      </ShellCard>
      <ShellCard title="MCP servers" description="Observed .mcp.json servers" icon={<Plug className="size-4" />}>
        {mcpServers.length === 0 ? <EmptyState>No .mcp.json servers observed.</EmptyState> : (
          <div className="space-y-2">
            {mcpServers.map((server) => (
              <div key={server.name} className="rounded-md border bg-muted/30 p-3 text-sm">
                <div className="font-medium">{server.name}</div>
                <code className="mt-1 block break-all text-xs text-muted-foreground">{[server.command, ...server.args].filter(Boolean).join(" ")}</code>
              </div>
            ))}
          </div>
        )}
      </ShellCard>
      <ShellCard title="Sessions" description="Claude, Codex, Gemini, and Pi sessions" icon={<History className="size-4" />} className="lg:col-span-2">
        {sessions.length === 0 ? <EmptyState>No native agent sessions observed for this project.</EmptyState> : (
          <div className="space-y-2">
            {sessions.map((session, index) => (
              <div key={`${session.agent}-${session.id ?? index}-${session.path}`} className="rounded-md border bg-muted/30 p-3 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge>{session.agent}</Badge>
                  {session.id ? <code className="text-xs text-muted-foreground">{session.id}</code> : null}
                  <span className="ml-auto text-xs text-muted-foreground">{session.started_at}</span>
                </div>
                {session.first_message ? <p className="mt-2 text-sm">{session.first_message}</p> : null}
                {session.path ? <code className="mt-2 block truncate text-xs text-muted-foreground">{session.path}</code> : null}
              </div>
            ))}
          </div>
        )}
      </ShellCard>
    </div>
  );
}

function InboxPanel({ project }: { project: ProjectObservation }) {
  const installCommand = "pnpm dlx @whaleen/agent-inbox init --adapter tauri";
  return (
    <ShellCard title="Agent inbox" description={project.inbox.installed ? "agent-inbox observed" : "not installed"} icon={<Inbox className="size-4" />}>
      <div className="grid grid-cols-3 gap-2">
        {ALL_STATUSES.map((status) => (
          <div key={status} className="rounded-md border bg-muted/30 p-3">
            <div className="text-xl font-semibold">{project.inbox.counts[status] ?? 0}</div>
            <div className="text-xs text-muted-foreground">{status.replace("_", " ")}</div>
          </div>
        ))}
      </div>
      <Separator className="my-4" />
      {project.inbox.latest_body ? (
        <div className="space-y-1">
          <div className="text-sm font-medium">{project.inbox.latest_title ?? "Latest inbox item"}</div>
          <p className="line-clamp-3 text-sm text-muted-foreground">{project.inbox.latest_body}</p>
        </div>
      ) : (
        <div className="space-y-2 rounded-md border border-dashed p-3">
          <div className="text-xs text-muted-foreground">Install command</div>
          <code className="block text-xs text-foreground">{installCommand}</code>
        </div>
      )}
    </ShellCard>
  );
}

function FreshnessBadge({ freshness }: { freshness?: GitHubFreshness }) {
  if (!freshness) return null;
  return <Badge variant={freshness.stale ? "destructive" : "outline"}>updated {fmtEpoch(freshness.fetched_at)}</Badge>;
}

function GitHubRepoPanel({ project, repoData, freshness, refreshing, onRefresh }: { project: ProjectObservation; repoData: GitHubRepoRecord | null; freshness?: GitHubFreshness; refreshing: boolean; onRefresh: () => void }) {
  return (
    <ShellCard title="GitHub repository" description={project.github_issues.repo ?? "no GitHub remote"} icon={<GitPullRequest className="size-4" />} className="lg:col-span-2">
      {project.github_issues.repo ? (
        <div className="space-y-4">
          <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="flex gap-4">
              <RepoAvatar project={project} repoData={repoData} size="lg" />
              <div className="min-w-0 flex-1">
                <p className="text-sm text-muted-foreground">{repoData?.description ?? "No cached GitHub repository metadata observed."}</p>
                <div className="mt-3 flex flex-wrap gap-2">
                  <Button size="sm" variant="outline" onClick={onRefresh} disabled={refreshing}>
                    <RefreshCw className={cn("size-3.5", refreshing && "animate-spin")} />
                    Refresh
                  </Button>
                  <FreshnessBadge freshness={freshness} />
                  {freshness?.error ? <Badge variant="destructive">{freshness.error}</Badge> : null}
                  {repoData?.url ? <Button size="sm" variant="outline" asChild><a href={repoData.url}>GitHub</a></Button> : null}
                  {repoData?.homepage_url ? <Button size="sm" variant="outline" asChild><a href={repoData.homepage_url}>Deployed site</a></Button> : null}
                </div>
              </div>
            </div>
            {repoData?.repo_image_url ? (
              <a href={repoData.url ?? repoData.repo_image_url} className="block overflow-hidden rounded-lg border bg-muted" aria-label="GitHub OpenGraph card">
                <img src={repoData.repo_image_url} alt="GitHub OpenGraph card" className="aspect-[1.91/1] w-full object-cover" />
              </a>
            ) : null}
          </div>
          <div className="grid gap-2 sm:grid-cols-3 xl:grid-cols-6">
            <StatCard label="Stars" value={repoData?.stars ?? "—"} />
            <StatCard label="Forks" value={repoData?.forks ?? "—"} />
            <StatCard label="Open PRs" value={repoData?.open_prs ?? "—"} />
            <StatCard label="Branch" value={repoData?.default_branch ?? "—"} />
            <StatCard label="License" value={repoData?.license ?? "—"} />
            <StatCard label="Pushed" value={repoData?.pushed_at?.slice(0, 10) ?? "—"} />
          </div>
          {repoData?.topics?.length ? (
            <div className="flex flex-wrap gap-1.5">{repoData.topics.map((topic) => <Badge key={topic} variant="secondary">{topic}</Badge>)}</div>
          ) : null}
        </div>
      ) : (
        <div className="rounded-md border border-dashed p-4 text-sm text-muted-foreground">No GitHub origin remote observed for this project.</div>
      )}
    </ShellCard>
  );
}

function GitHubIssuesPanel({ project, issueCount, freshness, refreshing, onRefresh }: { project: ProjectObservation; issueCount: number; freshness?: GitHubFreshness; refreshing: boolean; onRefresh: () => void }) {
  return (
    <ShellCard title="GitHub issues" description={project.github_issues.repo ?? "no GitHub remote"} icon={<GitPullRequest className="size-4" />}>
      <div className="text-3xl font-semibold tracking-tight">{issueCount}</div>
      <p className="mt-2 text-sm text-muted-foreground">
        {project.github_issues.error
          ? project.github_issues.error
          : project.github_issues.available
            ? "Open GitHub issues are tracked alongside local agent inbox records."
            : "No GitHub issue data observed for this project."}
      </p>
      {project.github_issues.available ? <div className="mt-3 flex flex-wrap gap-2"><Button size="sm" variant="outline" onClick={onRefresh} disabled={refreshing}><RefreshCw className={cn("size-3.5", refreshing && "animate-spin")} />Refresh</Button><FreshnessBadge freshness={freshness} />{freshness?.error ? <Badge variant="destructive">{freshness.error}</Badge> : null}</div> : null}
    </ShellCard>
  );
}

function GitHealthPanel({ summary }: { summary: GitSummary | null }) {
  return (
    <ShellCard title="Git health" description="Selected project local git state" icon={<FolderGit2 className="size-4" />}>
      <div className="grid grid-cols-2 gap-2">
        <StatCard label="Branch" value={summary?.branch ?? "—"} />
        <StatCard label="Dirty" value={summary?.dirty_count ?? "—"} />
        <StatCard label="Ahead" value={summary?.ahead ?? "—"} />
        <StatCard label="Behind" value={summary?.behind ?? "—"} />
      </div>
      <Separator className="my-4" />
      <div className="text-xs text-muted-foreground">Last commit</div>
      <div className="mt-1 truncate text-sm font-mono">{summary?.last_commit ?? "—"}</div>
    </ShellCard>
  );
}

function SuggestedActionsPanel({ project, inboxRecords, issueCount }: { project: ProjectObservation; inboxRecords: AgentInboxRecord[]; issueCount: number }) {
  const [copied, setCopied] = React.useState<string | null>(null);
  const hasContext = (path: string) => project.context_files.some((item) => item.path === path && item.present);
  const actions: { id: string; title: string; description: string; prompt: string }[] = [];

  if (!hasContext("SPEC.md")) actions.push({ id: "missing-spec", title: "Draft SPEC.md", description: "Create a product contract for this project.", prompt: buildMissingSpecPrompt(project) });
  if (!hasContext("AGENTS.md")) actions.push({ id: "missing-agents", title: "Draft AGENTS.md", description: "Create durable coding-agent context.", prompt: buildMissingAgentsPrompt(project) });
  if (!project.inbox.installed) actions.push({ id: "install-inbox", title: "Copy agent-inbox install", description: "Install the source-based agent inbox package.", prompt: `cd ${project.path}\npnpm dlx @whaleen/agent-inbox init --adapter tauri` });
  if ((project.inbox.counts.new ?? 0) > 0) actions.push({ id: "plan-inbox", title: "Plan new agent inbox records", description: "Plan all new records and write plans back.", prompt: buildAgentInboxPlanAllPrompt(project) });
  if ((project.inbox.counts.accepted ?? 0) > 0) actions.push({ id: "implement-inbox", title: "Implement accepted agent inbox records", description: "Work accepted records through done.", prompt: buildAcceptedInboxPrompt(project) });
  if (issueCount > 0) actions.push({ id: "triage-issues", title: "Triage GitHub issues", description: "Plan or group open GitHub issues.", prompt: buildIssueTriagePrompt(project) });
  if (inboxRecords.some((record) => record.status === "planned")) actions.push({ id: "review-plans", title: "Review planned agent inbox records", description: "Check planned records for acceptance readiness.", prompt: `Review planned agent inbox records in ${project.path}. Read .agent/inbox/README.md, inspect records with status planned, and summarize which are ready for user acceptance vs need revision.` });

  async function copyAction(action: { id: string; title: string; prompt: string }) {
    await navigator.clipboard.writeText(action.prompt);
    setCopied(action.id);
    window.setTimeout(() => setCopied(null), 1600);
  }

  return (
    <ShellCard title="Suggested actions" description="Copyable read-only recipes from observed state" icon={<Clipboard className="size-4" />}>
      <div className="space-y-2">
        {actions.length === 0 ? <EmptyState>No suggested actions for this project right now.</EmptyState> : actions.map((action) => (
          <div key={action.id} className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 p-3">
            <div className="min-w-0">
              <div className="font-medium">{action.title}</div>
              <div className="text-sm text-muted-foreground">{action.description}</div>
            </div>
            <Button size="sm" variant="outline" onClick={() => copyAction(action)}>{copied === action.id ? "Copied" : "Copy"}</Button>
          </div>
        ))}
      </div>
    </ShellCard>
  );
}

function AgentLibraryPanel({ library }: { library: AgentLibraryOverview }) {
  const [copied, setCopied] = React.useState<string | null>(null);
  const assets = [...library.recipes, ...library.prompts, ...library.skills];
  async function copyAsset(asset: LibraryAsset) {
    await navigator.clipboard.writeText(asset.body);
    setCopied(asset.path);
    window.setTimeout(() => setCopied(null), 1600);
  }
  return (
    <ShellCard title="Agent Library" description={library.root} icon={<Bot className="size-4" />} className="lg:col-span-2">
      <div className="mb-4 grid grid-cols-3 gap-2">
        <StatCard label="Recipes" value={library.recipes.length} />
        <StatCard label="Prompts" value={library.prompts.length} />
        <StatCard label="Skills" value={library.skills.length} />
      </div>
      <div className="space-y-2">
        {assets.length === 0 ? <EmptyState>No agent-library assets observed.</EmptyState> : assets.map((asset) => (
          <div key={asset.path} className="flex items-center justify-between gap-3 rounded-lg border bg-muted/20 p-3">
            <div className="min-w-0">
              <div className="flex items-center gap-2"><Badge variant="secondary">{asset.kind}</Badge><span className="font-medium">{asset.name}</span></div>
              <div className="mt-1 text-sm text-muted-foreground">{asset.description ?? asset.path}</div>
              {asset.tags.length ? <div className="mt-2 flex flex-wrap gap-1">{asset.tags.map((tag) => <Badge key={tag} variant="outline">{tag}</Badge>)}</div> : null}
            </div>
            <Button size="sm" variant="outline" onClick={() => copyAsset(asset)}>{copied === asset.path ? "Copied" : "Copy"}</Button>
          </div>
        ))}
      </div>
    </ShellCard>
  );
}

function AgentInboxDashboard({ records, selectedProjectPath }: { records: AgentInboxRecord[]; selectedProjectPath: string | null }) {
  const [filter, setFilter] = React.useState("active");
  const [selectedId, setSelectedId] = React.useState<string | null>(records[0]?.id ?? null);
  const [copyMessage, setCopyMessage] = React.useState<string | null>(null);

  const filteredRecords = records.filter((record) => {
    if (filter === "active") return ACTIVE_STATUSES.includes(record.status);
    if (filter === "project") return record.project_path === selectedProjectPath;
    return record.status === filter;
  });
  const selected = filteredRecords.find((record) => record.id === selectedId) ?? filteredRecords[0] ?? null;

  React.useEffect(() => {
    if (filteredRecords.length > 0 && !filteredRecords.some((record) => record.id === selectedId)) setSelectedId(filteredRecords[0].id);
  }, [filteredRecords, selectedId]);

  async function copyPrompt(prompt: string, label: string) {
    await navigator.clipboard.writeText(prompt);
    setCopyMessage(`${label} copied`);
    window.setTimeout(() => setCopyMessage(null), 1600);
  }

  return (
    <ShellCard title="Agent inbox" description={`${records.length} active records observed`} icon={<MessageSquareText className="size-4" />} className="lg:col-span-2">
      <div className="mb-4 flex flex-wrap items-center gap-2">
        {["active", "project", ...ACTIVE_STATUSES].map((item) => <Button key={item} variant={filter === item ? "secondary" : "outline"} size="sm" onClick={() => setFilter(item)}>{item.replace("_", " ")}</Button>)}
        {copyMessage ? <span className="ml-auto text-sm text-chart-3">{copyMessage}</span> : null}
      </div>
      <div className="grid gap-4 lg:grid-cols-[minmax(260px,0.8fr)_minmax(0,1.2fr)]">
        <div className="max-h-[520px] space-y-2 overflow-auto pr-1">
          {filteredRecords.length === 0 ? <EmptyState>No inbox records match this filter.</EmptyState> : filteredRecords.map((record) => (
            <Button key={`${record.project_path}:${record.id}`} variant={selected?.id === record.id ? "secondary" : "ghost"} className="h-auto w-full justify-start p-3 text-left" onClick={() => setSelectedId(record.id)}>
              <div className="min-w-0 space-y-1">
                <div className="flex items-center gap-2 text-xs text-muted-foreground"><span>{record.project_name}</span><Badge variant="outline">{record.status.replace("_", " ")}</Badge></div>
                <div className="truncate font-medium">{recordTitle(record)}</div>
                <p className="line-clamp-2 text-xs font-normal text-muted-foreground">{record.body}</p>
              </div>
            </Button>
          ))}
        </div>
        <RecordDetail
          selected={selected}
          onCopyPlan={(record) => copyPrompt(buildPlanPrompt(record), "Plan prompt")}
          onCopyImplement={(record) => copyPrompt(buildImplementPrompt(record), "Implement prompt")}
        />
      </div>
    </ShellCard>
  );
}

function RecordDetail({ selected, onCopyPlan, onCopyImplement }: { selected: AgentInboxRecord | null; onCopyPlan: (record: AgentInboxRecord) => void; onCopyImplement: (record: AgentInboxRecord) => void }) {
  if (!selected) return <EmptyState>Select a record to inspect its request, plan, context, and prompts.</EmptyState>;
  return (
    <div className="min-h-[420px] rounded-lg border bg-muted/20 p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <div className="text-xs uppercase tracking-wide text-muted-foreground">{selected.project_name} / {selected.kind}</div>
          <h3 className="mt-1 text-xl font-semibold tracking-tight">{recordTitle(selected)}</h3>
        </div>
        <Badge>{selected.status.replace("_", " ")}</Badge>
      </div>
      <Separator className="my-4" />
      <div className="space-y-4">
        <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">User request</div><MarkdownBlock>{selected.body}</MarkdownBlock></section>
        {selected.plan ? <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">Saved plan</div><pre className="max-h-48 overflow-auto rounded-md bg-background p-3 text-xs whitespace-pre-wrap">{selected.plan}</pre></section> : null}
        {selected.comments ? <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">Comments</div><pre className="max-h-32 overflow-auto rounded-md bg-background p-3 text-xs whitespace-pre-wrap">{JSON.stringify(selected.comments, null, 2)}</pre></section> : null}
        {selected.agent_notes ? <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">Agent notes</div><pre className="max-h-32 overflow-auto rounded-md bg-background p-3 text-xs whitespace-pre-wrap">{JSON.stringify(selected.agent_notes, null, 2)}</pre></section> : null}
        {selected.context ? <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">Context</div><pre className="max-h-32 overflow-auto rounded-md bg-background p-3 text-xs whitespace-pre-wrap">{JSON.stringify(selected.context, null, 2)}</pre></section> : null}
        <div className="grid gap-2 sm:grid-cols-3">
          <MetaBox label="ID" value={selected.id} />
          <MetaBox label="Route" value={selected.route ?? "—"} />
          <MetaBox label="Updated" value={selected.updated_at ?? selected.created_at ?? "—"} />
        </div>
        <div className="flex flex-wrap gap-2"><Button size="sm" variant="outline" onClick={() => onCopyPlan(selected)}><Clipboard className="size-4" /> Copy plan prompt</Button><Button size="sm" variant="outline" onClick={() => onCopyImplement(selected)}><Bot className="size-4" /> Copy implement prompt</Button></div>
      </div>
    </div>
  );
}

function GitHubIssuesDashboard({ issues, selectedProjectPath }: { issues: GitHubIssueRecord[]; selectedProjectPath: string | null }) {
  const [filter, setFilter] = React.useState("all");
  const [selectedKey, setSelectedKey] = React.useState<string | null>(issues[0] ? `${issues[0].repo}#${issues[0].number}` : null);
  const [copyMessage, setCopyMessage] = React.useState<string | null>(null);
  const filteredIssues = issues.filter((issue) => filter === "all" || issue.project_path === selectedProjectPath);
  const selected = filteredIssues.find((issue) => `${issue.repo}#${issue.number}` === selectedKey) ?? filteredIssues[0] ?? null;

  async function copyPrompt(issue: GitHubIssueRecord) {
    await navigator.clipboard.writeText(buildIssuePrompt(issue));
    setCopyMessage("Issue prompt copied");
    window.setTimeout(() => setCopyMessage(null), 1600);
  }

  return (
    <ShellCard title="GitHub issues" description={`${issues.length} open issues observed`} icon={<GitPullRequest className="size-4" />} className="lg:col-span-2">
      <div className="mb-4 flex flex-wrap items-center gap-2"><Button variant={filter === "all" ? "secondary" : "outline"} size="sm" onClick={() => setFilter("all")}>all</Button><Button variant={filter === "project" ? "secondary" : "outline"} size="sm" onClick={() => setFilter("project")}>project</Button>{copyMessage ? <span className="ml-auto text-sm text-chart-3">{copyMessage}</span> : null}</div>
      <div className="grid gap-4 lg:grid-cols-[minmax(260px,0.8fr)_minmax(0,1.2fr)]">
        <div className="max-h-[520px] space-y-2 overflow-auto pr-1">
          {filteredIssues.length === 0 ? <EmptyState>No GitHub issues match this filter.</EmptyState> : filteredIssues.map((issue) => (
            <Button key={`${issue.repo}#${issue.number}`} variant={selected?.repo === issue.repo && selected?.number === issue.number ? "secondary" : "ghost"} className="h-auto w-full justify-start p-3 text-left" onClick={() => setSelectedKey(`${issue.repo}#${issue.number}`)}>
              <div className="min-w-0 space-y-1"><div className="flex items-center gap-2 text-xs text-muted-foreground"><span>{issue.project_name}</span><Badge variant="outline">#{issue.number}</Badge></div><div className="truncate font-medium">{issue.title}</div><p className="line-clamp-2 text-xs font-normal text-muted-foreground">{issue.body || "No issue body."}</p></div>
            </Button>
          ))}
        </div>
        {selected ? (
          <div className="min-h-[420px] rounded-lg border bg-muted/20 p-4">
            <div className="text-xs uppercase tracking-wide text-muted-foreground">{selected.repo}</div>
            <h3 className="mt-1 text-xl font-semibold tracking-tight">#{selected.number} {selected.title}</h3>
            <Separator className="my-4" />
            <section><div className="mb-1 text-xs uppercase tracking-wide text-muted-foreground">Issue body</div>{selected.body ? <MarkdownBlock>{selected.body}</MarkdownBlock> : <p className="text-sm text-muted-foreground">No issue body.</p>}</section>
            <div className="my-4 grid gap-2 sm:grid-cols-3"><MetaBox label="Project" value={selected.project_name} /><MetaBox label="Updated" value={selected.updated_at ?? "—"} /><MetaBox label="URL" value={selected.url ?? "—"} /></div>
            <div className="mb-4 flex flex-wrap gap-1.5">{selected.labels.map((label) => <Badge key={label} variant="secondary">{label}</Badge>)}</div>
            <Button size="sm" variant="outline" onClick={() => copyPrompt(selected)}><Clipboard className="size-4" /> Copy issue prompt</Button>
          </div>
        ) : <EmptyState>Select an issue to inspect it.</EmptyState>}
      </div>
    </ShellCard>
  );
}

function MetaBox({ label, value }: { label: string; value: React.ReactNode }) {
  return <div className="min-w-0 rounded-md border bg-background p-3"><div className="text-xs uppercase tracking-wide text-muted-foreground">{label}</div><div className="mt-1 truncate text-xs font-mono">{value}</div></div>;
}

function reactNodeText(node: React.ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(reactNodeText).join("");
  if (React.isValidElement<{ children?: React.ReactNode }>(node)) return reactNodeText(node.props.children);
  return "";
}

function CopyablePre({ children, ...props }: React.ComponentProps<"pre">) {
  const [copied, setCopied] = React.useState(false);
  const code = reactNodeText(children).replace(/\n$/, "");

  async function copyCode() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  }

  return (
    <div className="group/code relative my-4">
      <Button
        type="button"
        size="sm"
        variant="secondary"
        className="absolute right-2 top-2 h-7 opacity-0 transition-opacity group-hover/code:opacity-100 focus:opacity-100"
        onClick={copyCode}
      >
        <Clipboard className="size-3.5" />
        {copied ? "Copied" : "Copy"}
      </Button>
      <pre {...props} className="overflow-auto rounded-lg border bg-muted p-4 pr-20 text-foreground">
        {children}
      </pre>
    </div>
  );
}

function MarkdownBlock({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn("prose prose-sm max-w-none dark:prose-invert prose-headings:scroll-m-20 prose-code:text-foreground prose-a:text-primary", className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ pre: CopyablePre }}>{children}</ReactMarkdown>
    </div>
  );
}

function ReadmePanel({ project }: { project: ProjectObservation }) {
  return (
    <ShellCard title="README" description="Project documentation" icon={<MessageSquareText className="size-4" />} className="lg:col-span-2">
      {project.readme ? <MarkdownBlock className="max-h-[520px] overflow-auto pr-2">{project.readme}</MarkdownBlock> : <EmptyState>No README observed for this project.</EmptyState>}
    </ShellCard>
  );
}

function EmptyState({ children }: React.PropsWithChildren) {
  return <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">{children}</div>;
}

function App() {
  const [overview, setOverview] = React.useState<AppOverview | null>(null);
  const [overviewRefreshing, setOverviewRefreshing] = React.useState(false);
  const [lastLocalObservationAt, setLastLocalObservationAt] = React.useState<number>(0);
  // GitHub issues: keyed by project_path -> { records, freshness }
  const [githubIssuesMap, setGithubIssuesMap] = React.useState<Record<string, GitHubIssuesResponse>>({});
  // GitHub repo: keyed by project_path -> { record, freshness }
  const [githubReposMap, setGithubReposMap] = React.useState<Record<string, GitHubRepoResponse>>({});
  // Which paths have had initial cache-load attempted (no live call, just cache query)
  const [githubIssuesCacheLoaded, setGithubIssuesCacheLoaded] = React.useState<Set<string>>(new Set());
  const [githubRepoCacheLoaded, setGithubRepoCacheLoaded] = React.useState<Set<string>>(new Set());
  // Per-path refreshing state for UI indicators
  const [githubIssuesRefreshing, setGithubIssuesRefreshing] = React.useState<Set<string>>(new Set());
  const [githubRepoRefreshing, setGithubRepoRefreshing] = React.useState<Set<string>>(new Set());
  const [gitSummaries, setGitSummaries] = React.useState<Record<string, GitSummary>>({});
  const [gitLoadedPaths, setGitLoadedPaths] = React.useState<string[]>([]);
  const [projectAgents, setProjectAgents] = React.useState<Record<string, ProjectAgentsOverview>>({});
  const [agentsLoadedPaths, setAgentsLoadedPaths] = React.useState<string[]>([]);
  const [selectedPath, setSelectedPath] = React.useState<string | null>(null);
  const [projectSearch, setProjectSearch] = React.useState("");
  const [view, setView] = React.useState<"dashboard" | "agent-library" | "project">("dashboard");
  const [dashboardTab, setDashboardTab] = React.useState<DashboardTab>("overview");
  const [projectTab, setProjectTab] = React.useState<ProjectTab>("overview");
  const [error, setError] = React.useState<string | null>(null);

  const refreshLocalOverview = React.useCallback((seedGithubIssues = false) => {
    setOverviewRefreshing(true);
    invoke<AppOverview>("app_overview")
      .then((data) => {
        setOverview(data);
        if (seedGithubIssues) {
          // Seed issue counts from cached data bundled into overview. This never calls GitHub.
          const issuesByPath: Record<string, GitHubIssuesResponse> = {};
          const seededPaths = new Set<string>();
          for (const record of data.github_issue_records ?? []) {
            const p = record.project_path;
            if (!issuesByPath[p]) {
              issuesByPath[p] = { records: [], freshness: { fetched_at: 0, stale: true, source: "github-cache", error: null } };
            }
            issuesByPath[p].records.push(record);
            seededPaths.add(p);
          }
          setGithubIssuesMap(issuesByPath);
          setGithubIssuesCacheLoaded(seededPaths);
        }
        setSelectedPath((current) => current && data.projects.some((project) => project.path === current) ? current : data.projects[0]?.path ?? null);
        setError(null);
      })
      .catch((err) => setError(String(err)))
      .finally(() => setOverviewRefreshing(false));
  }, []);

  React.useEffect(() => {
    refreshLocalOverview(true);
  }, [refreshLocalOverview]);

  React.useEffect(() => {
    const interval = window.setInterval(() => {
      if (document.visibilityState === "visible") refreshLocalOverview(false);
    }, 60_000);
    return () => window.clearInterval(interval);
  }, [refreshLocalOverview]);

  React.useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<LocalObservationEvent>("observation://local-changed", (event) => {
      setLastLocalObservationAt(event.payload.observed_at);
      if (document.visibilityState === "visible") refreshLocalOverview(false);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshLocalOverview]);

  // When navigating to a project, load cached GitHub data (no live call) if not yet loaded.
  React.useEffect(() => {
    if (!overview || !selectedPath || githubIssuesCacheLoaded.has(selectedPath)) return;
    const path = selectedPath;
    const project = overview.projects.find((item) => item.path === path);
    if (!project?.github_issues.repo) return;
    let cancelled = false;
    invoke<GitHubIssuesResponse>("inspect_github_issues", { path }).then((res) => {
      if (!cancelled) setGithubIssuesMap((prev) => ({ ...prev, [path]: res }));
    }).catch(() => {}).finally(() => {
      if (!cancelled) setGithubIssuesCacheLoaded((prev) => new Set([...prev, path]));
    });
    return () => { cancelled = true; };
  }, [overview, selectedPath, githubIssuesCacheLoaded]);

  React.useEffect(() => {
    if (!overview || !selectedPath || githubRepoCacheLoaded.has(selectedPath)) return;
    const path = selectedPath;
    const project = overview.projects.find((item) => item.path === path);
    if (!project?.github_issues.repo) return;
    let cancelled = false;
    invoke<GitHubRepoResponse>("inspect_github_repo", { path }).then((res) => {
      if (!cancelled) setGithubReposMap((prev) => ({ ...prev, [path]: res }));
    }).catch(() => {}).finally(() => {
      if (!cancelled) setGithubRepoCacheLoaded((prev) => new Set([...prev, path]));
    });
    return () => { cancelled = true; };
  }, [overview, selectedPath, githubRepoCacheLoaded]);

  // Explicit user-triggered refresh (live GitHub call)
  function handleRefreshGithubIssues(path: string) {
    if (githubIssuesRefreshing.has(path)) return;
    setGithubIssuesRefreshing((prev) => new Set([...prev, path]));
    invoke<GitHubIssuesResponse>("refresh_github_issues", { path })
      .then((res) => {
        setGithubIssuesMap((prev) => ({ ...prev, [path]: res }));
        setGithubIssuesCacheLoaded((prev) => new Set([...prev, path]));
      })
      .catch(() => {})
      .finally(() => setGithubIssuesRefreshing((prev) => { const next = new Set(prev); next.delete(path); return next; }));
  }

  function handleRefreshGithubRepo(path: string) {
    if (githubRepoRefreshing.has(path)) return;
    setGithubRepoRefreshing((prev) => new Set([...prev, path]));
    invoke<GitHubRepoResponse>("refresh_github_repo", { path })
      .then((res) => {
        setGithubReposMap((prev) => ({ ...prev, [path]: res }));
        setGithubRepoCacheLoaded((prev) => new Set([...prev, path]));
      })
      .catch(() => {})
      .finally(() => setGithubRepoRefreshing((prev) => { const next = new Set(prev); next.delete(path); return next; }));
  }

  React.useEffect(() => {
    if (!overview || view !== "project" || !selectedPath || gitLoadedPaths.includes(selectedPath)) return;
    const path = selectedPath;
    let cancelled = false;
    async function loadGitSummary() {
      try {
        const summary = await invoke<GitSummary>("inspect_git_summary", { path });
        if (!cancelled) setGitSummaries((prev) => ({ ...prev, [path]: summary }));
      } finally {
        if (!cancelled) setGitLoadedPaths((prev) => prev.includes(path) ? prev : [...prev, path]);
      }
    }
    loadGitSummary();
    return () => { cancelled = true; };
  }, [overview, view, selectedPath, gitLoadedPaths]);

  React.useEffect(() => {
    if (!overview || view !== "project" || !selectedPath || agentsLoadedPaths.includes(selectedPath)) return;
    if (projectTab !== "memories" && projectTab !== "agents") return;
    const path = selectedPath;
    let cancelled = false;
    async function loadProjectAgents() {
      try {
        const data = await invoke<ProjectAgentsOverview>("inspect_project_agents", { path });
        if (!cancelled) setProjectAgents((prev) => ({ ...prev, [path]: data }));
      } finally {
        if (!cancelled) setAgentsLoadedPaths((prev) => prev.includes(path) ? prev : [...prev, path]);
      }
    }
    loadProjectAgents();
    return () => { cancelled = true; };
  }, [overview, view, selectedPath, projectTab, agentsLoadedPaths]);

  const issueCountsByPath = React.useMemo(() => {
    const counts = new Map<string, number>();
    for (const [path, resp] of Object.entries(githubIssuesMap)) {
      counts.set(path, resp.records.length);
    }
    return counts;
  }, [githubIssuesMap]);

  if (error) return <main className="grid h-full place-items-center"><EmptyState>{error}</EmptyState></main>;
  if (!overview) return <main className="grid h-full place-items-center text-sm text-muted-foreground">Loading project-index observations…</main>;

  const selected = overview.projects.find((project) => project.path === selectedPath) ?? overview.projects[0];
  const search = projectSearch.trim().toLowerCase();
  const filteredProjects = overview.projects.filter((project) => !search || project.name.toLowerCase().includes(search) || project.path.toLowerCase().includes(search));
  const groupedProjects = filteredProjects.reduce<Record<string, ProjectObservation[]>>((groups, project) => {
    const parts = project.path.split("/").filter(Boolean);
    const group = parts.length >= 2 ? parts[parts.length - 2] : "Projects";
    groups[group] = groups[group] ?? [];
    groups[group].push(project);
    return groups;
  }, {});
  const inboxProjects = overview.projects.filter((project) => project.inbox.installed).length;
  const activeInbox = overview.inbox_records.length;
  const allGithubIssues = Object.values(githubIssuesMap).flatMap((response) => response.records);
  const openIssues = allGithubIssues.length;
  const selectedInboxRecords = overview.inbox_records.filter((record) => record.project_path === selected?.path);
  const selectedGithubIssuesResponse = selected ? githubIssuesMap[selected.path] ?? null : null;
  const selectedGithubRepoResponse = selected ? githubReposMap[selected.path] ?? null : null;
  const selectedGithubIssues = selectedGithubIssuesResponse?.records ?? [];
  const selectedGithubRepo = selectedGithubRepoResponse?.record ?? null;
  const selectedGithubIssuesRefreshing = selected ? githubIssuesRefreshing.has(selected.path) : false;
  const selectedGithubRepoRefreshing = selected ? githubRepoRefreshing.has(selected.path) : false;
  const selectedGitSummary = selected ? gitSummaries[selected.path] ?? null : null;
  const selectedProjectAgents = selected ? projectAgents[selected.path] ?? null : null;
  const selectedContextOk = selected ? countPresent(selected) : 0;

  return (
    <SidebarProvider className="h-full min-h-0 overflow-hidden">
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <div className="flex items-center gap-3 px-2 py-1">
            <div className="grid size-9 place-items-center rounded-md bg-sidebar-primary text-sidebar-primary-foreground"><TerminalSquare className="size-5" /></div>
            <div className="min-w-0 group-data-[collapsible=icon]:hidden">
              <h1 className="font-semibold tracking-tight">project-index</h1>
              <p className="truncate text-xs text-muted-foreground">{overview.contract}</p>
            </div>
          </div>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton isActive={view === "dashboard"} onClick={() => setView("dashboard")} tooltip="Dashboard">
                <Radar className="size-4" />
                <span>Dashboard</span>
                <Badge className="ml-auto group-data-[collapsible=icon]:hidden">{activeInbox + openIssues}</Badge>
              </SidebarMenuButton>
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton isActive={view === "agent-library"} onClick={() => setView("agent-library")} tooltip="Agent Library">
                <Bot className="size-4" />
                <span>Agent Library</span>
                <Badge variant="outline" className="ml-auto group-data-[collapsible=icon]:hidden">{overview.agent_library.skills.length}</Badge>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
          <div className="px-2 group-data-[collapsible=icon]:hidden">
            <div className="mb-2 rounded-md border bg-muted/30 p-2">
              <div className="mb-1 text-xs text-muted-foreground">Projects root</div>
              <code className="block truncate text-xs">{overview.projects_root}</code>
            </div>
            <div className="relative">
              <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input className="h-8 pl-9" value={projectSearch} onChange={(event) => setProjectSearch(event.target.value)} placeholder="Search projects…" />
            </div>
          </div>
        </SidebarHeader>
        <SidebarContent>
          {Object.entries(groupedProjects).map(([group, projects]) => (
            <SidebarGroup key={group}>
              <SidebarGroupLabel className="gap-2">
                <GroupAvatar group={group} />
                <span>{group}</span>
                <Badge variant="outline" className="ml-auto group-data-[collapsible=icon]:hidden">{projects.length}</Badge>
              </SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {projects.map((project) => (
                    <ProjectCard
                      key={project.path}
                      project={project}
                      issueCount={issueCountsByPath.get(project.path) ?? 0}
                      selected={view === "project" && project.path === selected?.path}
                      onSelect={() => { setSelectedPath(project.path); setView("project"); }}
                    />
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
          {filteredProjects.length === 0 ? <div className="px-2"><EmptyState>No projects match “{projectSearch}”.</EmptyState></div> : null}
        </SidebarContent>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="min-h-0 overflow-hidden">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="h-4" />
          <div className="truncate text-sm text-muted-foreground">{view === "dashboard" ? "Dashboard" : view === "agent-library" ? "Agent Library" : selected?.name}</div>
          {lastLocalObservationAt ? <Badge variant="outline" className="ml-auto hidden sm:inline-flex">local changed {fmtEpoch(lastLocalObservationAt)}</Badge> : null}
          <div className={lastLocalObservationAt ? "" : "ml-auto"}>
            <Button size="sm" variant="outline" onClick={() => refreshLocalOverview(false)} disabled={overviewRefreshing}>
              <RefreshCw className={cn("size-3.5", overviewRefreshing && "animate-spin")} />
              Refresh local
            </Button>
          </div>
        </header>
        <section className="min-h-0 flex-1 overflow-y-auto p-6">
        {view === "dashboard" || view === "agent-library" ? (
          <Card className="mb-5 py-4">
            <CardContent className="flex flex-col gap-3 px-4 lg:flex-row lg:items-center lg:justify-between">
              <div className="min-w-0">
                <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{view === "agent-library" ? "Library" : "Dashboard"}</div>
                <h2 className="truncate text-xl font-semibold tracking-tight">{view === "agent-library" ? "Agent Library" : "Projects by latest commit"}</h2>
                <p className="mt-1 truncate text-sm text-muted-foreground">{view === "agent-library" ? overview.agent_library.root : overview.projects_root}</p>
              </div>
              <div className="flex flex-wrap gap-2">
                <Badge variant="secondary">{overview.projects.length} repos</Badge>
                <Badge variant="secondary">{inboxProjects} inboxes</Badge>
                <Badge variant="outline">{activeInbox} agent inbox</Badge>
                <Badge variant="outline">{openIssues} issues</Badge>
              </div>
            </CardContent>
          </Card>
        ) : selected ? (
          <Card className="mb-5 overflow-hidden">
            <CardContent className="flex flex-col gap-5 p-5 lg:flex-row lg:items-start lg:justify-between">
              <div className="flex min-w-0 gap-4">
                <RepoAvatar project={selected} repoData={selectedGithubRepo} size="lg" />
                <div className="min-w-0">
                  <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Project</div>
                  <h2 className="truncate text-3xl font-semibold tracking-tight">{selected.name}</h2>
                  <p className="mt-1 truncate text-sm text-muted-foreground">{selected.path}</p>
                  <p className="mt-3 max-w-3xl text-sm leading-6 text-muted-foreground">
                    {selectedGithubRepo?.description ?? selected.github_issues.repo ?? "No GitHub repository metadata observed."}
                  </p>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Badge variant="secondary">{selectedContextOk}/{selected.context_files.length} context</Badge>
                    <Badge variant={selected.inbox.active_count > 0 ? "default" : "outline"}>{selected.inbox.active_count} agent inbox</Badge>
                    <Badge variant={selectedGithubIssues.length > 0 ? "default" : "outline"}>{selectedGithubIssues.length} issues</Badge>
                    <Badge variant="outline" className="gap-1.5">{selected.github_issues.repo ? <OrgAvatar project={selected} repoData={selectedGithubRepo} /> : null}{selected.github_issues.repo ?? "no github"}</Badge>
                  </div>
                </div>
              </div>
              <div className="flex shrink-0 flex-wrap gap-2">
                {selectedGithubRepo?.url ? <Button size="sm" variant="outline" asChild><a href={selectedGithubRepo.url}>GitHub</a></Button> : null}
                {selectedGithubRepo?.homepage_url ? <Button size="sm" variant="outline" asChild><a href={selectedGithubRepo.homepage_url}>Deployed site</a></Button> : null}
              </div>
            </CardContent>
          </Card>
        ) : null}

        {view === "dashboard" ? (
          <Tabs value={dashboardTab} onValueChange={(value) => setDashboardTab(value as DashboardTab)}>
            <TabsList><TabsTrigger value="overview">Projects</TabsTrigger><TabsTrigger value="agent-inbox">Agent Inbox</TabsTrigger><TabsTrigger value="github">GitHub</TabsTrigger></TabsList>
            <TabsContent value="overview" className="mt-4"><DashboardProjectGrid projects={filteredProjects} issueCountsByPath={issueCountsByPath} onSelect={(project) => { setSelectedPath(project.path); setView("project"); }} /></TabsContent>
            <TabsContent value="agent-inbox" className="mt-4"><AgentInboxDashboard records={overview.inbox_records} selectedProjectPath={selected?.path ?? null} /></TabsContent>
            <TabsContent value="github" className="mt-4"><GitHubIssuesDashboard issues={allGithubIssues} selectedProjectPath={selected?.path ?? null} /></TabsContent>
          </Tabs>
        ) : view === "agent-library" ? (
          <AgentLibraryPanel library={overview.agent_library} />
        ) : selected ? (
          <Tabs value={projectTab} onValueChange={(value) => setProjectTab(value as ProjectTab)}>
            <TabsList><TabsTrigger value="overview">Overview</TabsTrigger><TabsTrigger value="agent-inbox">Agent Inbox</TabsTrigger><TabsTrigger value="context">Context</TabsTrigger><TabsTrigger value="memories">Memories</TabsTrigger><TabsTrigger value="agents">Agents</TabsTrigger><TabsTrigger value="github">GitHub</TabsTrigger></TabsList>
            <TabsContent value="overview" className="mt-4 grid gap-4 lg:grid-cols-2"><GitHubRepoPanel project={selected} repoData={selectedGithubRepo} freshness={selectedGithubRepoResponse?.freshness} refreshing={selectedGithubRepoRefreshing} onRefresh={() => handleRefreshGithubRepo(selected.path)} /><ReadmePanel project={selected} /><SuggestedActionsPanel project={selected} inboxRecords={selectedInboxRecords} issueCount={selectedGithubIssues.length} /><GitHealthPanel summary={selectedGitSummary} /><InboxPanel project={selected} /><GitHubIssuesPanel project={selected} issueCount={issueCountsByPath.get(selected.path) ?? 0} freshness={selectedGithubIssuesResponse?.freshness} refreshing={selectedGithubIssuesRefreshing} onRefresh={() => handleRefreshGithubIssues(selected.path)} /></TabsContent>
            <TabsContent value="agent-inbox" className="mt-4"><AgentInboxDashboard records={selectedInboxRecords} selectedProjectPath={selected.path} /></TabsContent>
            <TabsContent value="context" className="mt-4"><ContextPanel project={selected} /></TabsContent>
            <TabsContent value="memories" className="mt-4"><MemoriesPanel agents={selectedProjectAgents} /></TabsContent>
            <TabsContent value="agents" className="mt-4"><AgentsPanel agents={selectedProjectAgents} /></TabsContent>
            <TabsContent value="github" className="mt-4 grid gap-4"><GitHubRepoPanel project={selected} repoData={selectedGithubRepo} freshness={selectedGithubRepoResponse?.freshness} refreshing={selectedGithubRepoRefreshing} onRefresh={() => handleRefreshGithubRepo(selected.path)} /><GitHubIssuesPanel project={selected} issueCount={selectedGithubIssues.length} freshness={selectedGithubIssuesResponse?.freshness} refreshing={selectedGithubIssuesRefreshing} onRefresh={() => handleRefreshGithubIssues(selected.path)} /><GitHubIssuesDashboard issues={selectedGithubIssues} selectedProjectPath={selected.path} /></TabsContent>
          </Tabs>
        ) : null}

        </section>
      </SidebarInset>
    </SidebarProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
