// 设置 → 小拾: model services (status, switch, stop), the embedding and rerank services, 使用记忆, 发给云端 (which modules'
// data a cloud model may get when 小拾 looks things up), cloud API keys (keychain
// only: shown as 已保存, never read back) and the cloud model list (a summary; 编辑 opens the list: drag to
// reorder, edit, delete, add). Rules live in Rust (src-tauri/src/picky).
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { GripVertical, Pencil, Plus, Trash2, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, Field, IconButton, Segmented, Switch, TextInput } from "@/shared/ui";
import {
  MEMORY_ENABLED_KEY,
  SERVICE_NAMES,
  pickyApi,
  usePickyStatus,
  useRefreshStatus,
  type CloudData,
  type CloudModel,
  type PickyStatus,
  type Service,
  type ServiceKind,
  type ServiceStatus,
  type Think,
} from "@/modules/picky/api";
import { THINK_LABELS } from "@/modules/picky/Thinking";
import { IssueNotice, ModelMenu, StatusDot } from "@/modules/picky/ModelMenu";
import { searchApi } from "@/shared/searchApi";
import { useSortable } from "@/shared/sortable";
import { Path, Row, Section } from "./layout";
import { StorageRow } from "./PickyStorage";

const HELP =
  "本地模型、向量和重排服务由 ~/Documents/LLM 里的脚本和模型清单管理，小拾只启动、切换和停止它们，不改里面的文件；同一时间只运行一个本地模型；退出柚儿园时，小拾启动的本地模型、向量和重排服务都会停掉。选云端模型时，这一轮的消息会发给那家服务。";
const SERVICE: Record<ServiceKind, { label: string; port: number; use: string }> = {
  embedding: { label: "向量服务", port: 8081, use: "挑选记忆、给资料建索引时" },
  rerank: { label: "重排服务", port: 8082, use: "给搜索结果重新排序时" },
};

function ServiceRow(props: { kind: ServiceKind; status: ServiceStatus }) {
  const toast = useToast();
  const refresh = useRefreshStatus();
  const { kind, status: s } = props;
  const meta = SERVICE[kind];
  const act = (a: "start" | "stop") => void pickyApi.service(kind, a).then(refresh, (e) => toast(errorText(e), "error"));
  const choose = (key: string) => void pickyApi.serviceModel(kind, key).then(refresh, (e) => toast(errorText(e), "error"));
  return (
    <>
      <Row
        label={meta.label}
        help={`127.0.0.1:${meta.port}，${meta.use}自动启动；小拾启动的闲置 30 分钟后自动停。模型从 ~/Documents/LLM 的模型清单里选${kind === "embedding" ? "，换了会在后台重算搜索用的向量" : ""}。`}
        actions={
          s.model && (
            <>
              <Dropdown<string> value={s.model.key} options={s.models.map((m) => ({ value: m.key, label: m.name }))} onChange={choose} ariaLabel={`${meta.label}的模型`} />
              {s.state === "stopped" ? (
                <Button disabled={s.working} onClick={() => act("start")}>
                  启动
                </Button>
              ) : (
                <Button disabled={s.working} onClick={() => act("stop")}>
                  停止
                </Button>
              )}
            </>
          )
        }
      >
        {s.model ? s.text : <span className="text-muted-foreground">模型清单里没有</span>}
      </Row>
      {s.issue && (
        <div className="border-b px-4 py-3">
          <IssueNotice issue={s.issue} compact />
        </div>
      )}
    </>
  );
}
const KEY_HELP = "Key 只存在这台 Mac 的钥匙串里，不进数据库、备份和导出，保存后也不会再显示出来。";

function KeyRow(props: { service: Service; saved: boolean }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const toast = useToast();
  const refresh = useRefreshStatus();
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await pickyApi.keySave(props.service, value);
      setValue("");
      setEditing(false);
      void refresh();
      toast(`已保存 ${SERVICE_NAMES[props.service]} 的 Key`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const remove = () => void pickyApi.keyRemove(props.service).then(refresh, (e) => toast(errorText(e), "error"));
  const label = SERVICE_NAMES[props.service];
  if (editing || !props.saved)
    return (
      <Row label={label} help={KEY_HELP}>
        <form onSubmit={save} className="flex items-center gap-2">
          <TextInput type="password" autoComplete="off" aria-label={`${label} API Key`} placeholder="API Key" value={value} onChange={(e) => setValue(e.target.value)} className="max-w-72 font-mono" />
          <Button type="submit" variant="primary" disabled={!value.trim()}>
            保存
          </Button>
          {editing && <Button onClick={() => (setEditing(false), setValue(""))}>取消</Button>}
        </form>
      </Row>
    );
  return (
    <Row
      label={label}
      help={KEY_HELP}
      actions={
        <>
          <Button onClick={() => setEditing(true)}>更新</Button>
          <Button variant="ghost-danger" onClick={remove}>
            移除
          </Button>
        </>
      }
    >
      Key 已保存
    </Row>
  );
}

const EMPTY: CloudModel = { id: "", name: "", apiModel: "", service: "deepSeek", contextWindow: 131072, maxOutputTokens: 8192, thinking: "switch" };

function CloudModelDialog(props: { model: CloudModel; onClose: () => void }) {
  const [m, setM] = useState(props.model);
  const [error, setError] = useState<string | null>(null);
  const refresh = useRefreshStatus();
  const num = (v: string) => Number(v.replace(/[^\d]/g, "")) || 0;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await pickyApi.cloudModelSave(m);
      void refresh();
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={props.model.id ? "编辑云端模型" : "新增云端模型"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel="保存">
      <Field label="名称">
        <TextInput autoFocus value={m.name} onChange={(e) => setM({ ...m, name: e.target.value })} />
      </Field>
      <Field label="服务">
        <Dropdown<Service>
          ariaLabel="服务"
          value={m.service}
          options={(Object.keys(SERVICE_NAMES) as Service[]).map((s) => ({ value: s, label: SERVICE_NAMES[s] }))}
          onChange={(service) => setM({ ...m, service })}
        />
      </Field>
      <Field label="接口模型名">
        <TextInput className="font-mono" value={m.apiModel} onChange={(e) => setM({ ...m, apiModel: e.target.value })} />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="上下文（令牌）">
          <TextInput inputMode="numeric" value={String(m.contextWindow)} onChange={(e) => setM({ ...m, contextWindow: num(e.target.value) })} />
        </Field>
        <Field label="回复上限（令牌）">
          <TextInput inputMode="numeric" value={String(m.maxOutputTokens)} onChange={(e) => setM({ ...m, maxOutputTokens: num(e.target.value) })} />
        </Field>
      </div>
      <Field label="思考">
        <div>
          <Segmented<Think>
            ariaLabel="思考"
            value={m.thinking}
            options={(["never", "switch", "always"] as Think[]).map((t) => ({ value: t, label: THINK_LABELS[t] }))}
            onChange={(thinking) => setM({ ...m, thinking })}
          />
        </div>
      </Field>
    </Modal>
  );
}

function CloudModels(props: { status: PickyStatus }) {
  const [open, setOpen] = useState(false);
  const list = props.status.cloud;
  return (
    <>
      <Row
        label="云端模型"
        actions={
          <Button onClick={() => setOpen(true)}>
            <Pencil size={14} strokeWidth={1.5} /> 编辑
          </Button>
        }
      >
        {list.length === 0 ? <span className="text-muted-foreground">还没有</span> : <span className="block truncate">{list.map((m) => m.name).join("、")}</span>}
      </Row>
      {open && <CloudModelsDialog models={list} onClose={() => setOpen(false)} />}
    </>
  );
}

const k = (n: number) => (n % 1024 === 0 ? `${n / 1024}K` : n.toLocaleString());

/** The cloud model list: drag a row to reorder (the model menu follows), edit, delete, add. */
function CloudModelsDialog(props: { models: CloudModel[]; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [editing, setEditing] = useState<CloudModel | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const toast = useToast();
  const refresh = useRefreshStatus();
  const sort = useSortable((order) => void pickyApi.cloudModelsReorder(order.map((i) => props.models[i].id)).then(refresh, (e) => toast(errorText(e), "error")));
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const remove = async (m: CloudModel) => {
    if (!(await confirm({ title: `删除「${m.name}」？`, confirmLabel: "删除", danger: true }))) return;
    pickyApi.cloudModelDelete(m.id).then(refresh, (e) => toast(errorText(e), "error"));
  };
  return (
    <dialog
      ref={ref}
      aria-label="云端模型"
      onCancel={(e) => {
        e.preventDefault();
        if (!editing) props.onClose();
      }}
      className="m-auto flex max-h-[70vh] w-[640px] max-w-[92vw] flex-col overflow-hidden rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30"
    >
      <div className="flex items-center gap-4 border-b px-5 py-3">
        <p className="text-base font-bold">云端模型</p>
        <div className="flex-1" />
        <IconButton label="关闭" onClick={props.onClose}>
          <X size={16} />
        </IconButton>
      </div>
      {/* sized by its rows (the dialog has only a max height), scrolling past it */}
      <div ref={sort.listRef} className="-m-1 min-h-0 flex-auto overflow-y-auto p-1 px-3 py-2">
        {props.models.length === 0 && <p className="px-2 py-6 text-center text-sm text-muted-foreground">还没有</p>}
        {props.models.map((m, i) => (
          <div key={m.id} {...sort.item(i)} className="rounded-control bg-popover">
            <div {...sort.handle(i)} className="group flex items-center gap-3 rounded-control px-2 py-2 text-sm hover:bg-muted/70">
              <GripVertical size={14} className="shrink-0 cursor-grab text-muted-foreground/60" />
              <span className="shrink-0">{m.name}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                {SERVICE_NAMES[m.service]} · <span className="font-mono">{m.apiModel}</span> · 上下文 {k(m.contextWindow)} · 回复 {k(m.maxOutputTokens)} · {THINK_LABELS[m.thinking]}
              </span>
              <span className="flex opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
                <IconButton label="编辑" onClick={() => setEditing(m)}>
                  <Pencil size={14} strokeWidth={1.5} />
                </IconButton>
                <IconButton label="删除" danger onClick={() => void remove(m)}>
                  <Trash2 size={14} strokeWidth={1.5} />
                </IconButton>
              </span>
            </div>
          </div>
        ))}
      </div>
      <div className="border-t p-2">
        <button type="button" onClick={() => setEditing(EMPTY)} className="flex items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
          <Plus size={14} /> 新增
        </button>
      </div>
      {editing && <CloudModelDialog model={editing} onClose={() => setEditing(null)} />}
      {confirmDialog}
    </dialog>
  );
}

const CLOUD_DATA_KEY = ["picky", "data-cloud"];

/** Which modules' data may go to cloud models (the rest only to the local model). */
function CloudDataRow() {
  const qc = useQueryClient();
  const toast = useToast();
  const list = useQuery({ queryKey: CLOUD_DATA_KEY, queryFn: pickyApi.dataCloud }).data;
  if (!list) return null;
  const toggle = (m: CloudData) =>
    void pickyApi.dataCloudSet(m.module, !m.allowed).then(
      () => qc.invalidateQueries({ queryKey: CLOUD_DATA_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <Row label="发给云端" help="选云端模型时，小拾只查选中的模块，查到的内容会发给那家服务；没选中的只给本地模型。芝麻开门和侧栏里隐藏的模块，小拾都不读。">
      <div className="flex flex-wrap gap-1.5">
        {list.map((m) => (
          <button
            key={m.module}
            type="button"
            aria-pressed={m.allowed}
            onClick={() => toggle(m)}
            className={`rounded-full border px-2.5 py-0.5 text-xs transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
              m.allowed ? "border-transparent bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-muted"
            }`}
          >
            {m.label}
          </button>
        ))}
      </div>
    </Row>
  );
}

const count = (n: number) => n.toLocaleString("zh-CN");

function SearchIndexRow() {
  const q = useQuery({
    queryKey: ["search", "index"],
    queryFn: searchApi.indexStatus,
    refetchInterval: (q) => (q.state.data?.filling || q.state.data?.rebuilding ? 1000 : 3000),
    refetchIntervalInBackground: true,
  });
  const s = q.data;
  if (!s) return null;
  const left = s.records - s.done;
  const text = s.rebuilding || s.filling ? `正在算向量 ${count(s.done)} / ${count(s.records)}` : left > 0 ? `${count(s.records)} 条 · ${count(left)} 条待补算` : `${count(s.records)} 条 · 已全部算好`;
  return (
    <Row
      label="搜索索引"
      help="从各模块的记录和小拾的对话算出来的搜索用的数据（按字找的索引和按意思找的向量；芝麻开门不算进去），单独存在数据目录的 search.db 里，不进备份和导出，删掉会自动重建。改过的记录，在按意思搜索时或向量服务开着时补算向量。"
    >
      <span className="flex flex-col">
        <span>
          {text}
          <span className="text-xs text-muted-foreground"> · {(s.bytes / 1048576).toFixed(1)} MB</span>
        </span>
        {s.error && <span className="text-xs text-destructive">{s.error}</span>}
      </span>
    </Row>
  );
}

export function PickySection() {
  const status = usePickyStatus().data;
  const info = useQuery({ queryKey: ["picky", "info"], queryFn: pickyApi.info }).data;
  const memoryOn = useQuery({ queryKey: MEMORY_ENABLED_KEY, queryFn: pickyApi.memoryEnabled });
  const qc = useQueryClient();
  const toast = useToast();
  if (!status) return null;
  const local = status.local;
  return (
    <Section title="小拾" help={HELP}>
      {status.mode === "mock" && (
        <Row label="模型" help="开发版只用模拟的模型：不启动、不切换你的模型，不连真实接口，也不碰钥匙串。">
          <span className="text-muted-foreground">模拟</span>
        </Row>
      )}
      <Row label="本地模型" actions={status.error ? undefined : <ModelMenu status={{ ...status, selection: "local", cloud: [] }} />}>
        <span className="flex items-center gap-2">
          <StatusDot dot={local.presentation.dot} />
          {local.presentation.title}
          {local.active && <span className="text-xs text-muted-foreground">{local.active.quant} · 上下文 {Math.round(local.active.context / 1024)}K</span>}
        </span>
      </Row>
      {local.issue && (
        <div className="border-b px-4 py-3">
          <IssueNotice issue={local.issue} compact />
        </div>
      )}
      <ServiceRow kind="embedding" status={status.embedding} />
      <ServiceRow kind="rerank" status={status.rerank} />
      <SearchIndexRow />
      {memoryOn.data != null && (
        <Row
          label="使用记忆"
          help="发送时按问题从可用的记忆里挑选相关的几条（要用向量服务）。关掉后所有对话都不用记忆，记下的内容还在。"
          actions={
            <Switch
              label="使用记忆"
              checked={memoryOn.data}
              onChange={(on) =>
                void pickyApi.memoryEnabledSet(on).then(
                  () => qc.invalidateQueries({ queryKey: ["picky"] }),
                  (e) => toast(errorText(e), "error"),
                )
              }
            />
          }
        >
          <span className="text-muted-foreground">{memoryOn.data ? "开" : "关"}</span>
        </Row>
      )}
      <CloudDataRow />
      {status.keys.map((k) => (
        <KeyRow key={k.service} service={k.service} saved={k.saved} />
      ))}
      <CloudModels status={status} />
      {info && (
        <Row label="数据库" help="小拾的对话和设置单独存在这个文件里，和生活数据分开备份、分开恢复。">
          <Path value={info.dbPath} />
          <span className="ml-2 text-xs text-muted-foreground">版本 {info.schemaVersion ?? "—"}</span>
        </Row>
      )}
      <StorageRow />
    </Section>
  );
}
