// 设置 → 柚子: model services (status, switch, stop), the embedding and rerank services, 使用记忆, 发给云端 (which modules'
// data a cloud model may get when 柚子 looks things up), cloud API keys (keychain
// only: shown as 已保存, never read back) and the cloud model list (a summary; 编辑 opens the list: drag to
// reorder, edit, delete, add). Rules live in Rust (src-tauri/src/youzi).
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, GripVertical, Pencil, Plus, Trash2, X } from "@/shared/icons";
import { Dropdown, type DropdownOption } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, Field, IconButton, Segmented, Switch, TextInput } from "@/shared/ui";
import {
  MEMORY_ENABLED_KEY,
  SERVICE_NAMES,
  youziApi,
  useYouziStatus,
  useRefreshStatus,
  type CloudData,
  type CloudModel,
  type Level,
  type YouziStatus,
  type Service,
  type ServiceKind,
  type ServiceStatus,
  type Think,
} from "@/modules/youzi/api";
import { THINK_LABELS, levelFor, levelsOf, thinkOf } from "@/modules/youzi/Thinking";
import { IssueNotice, ModelMenu, StatusDot, modelOptions } from "@/modules/youzi/ModelMenu";
import { searchApi } from "@/shared/searchApi";
import { isStringArray, useSetting } from "@/shared/settings";
import { useSortable } from "@/shared/sortable";
import { Path, Row, Section } from "./layout";
import { StorageRow } from "./YouziStorage";

const HELP =
  "本地模型、向量和重排服务由 ~/Documents/LLM 里的脚本和模型清单管理，柚子只启动、切换和停止它们，不改里面的文件；同一时间只运行一个本地模型；退出柚儿园时，柚子启动的本地模型、向量和重排服务都会停掉。选云端模型时，这一轮的消息会发给那家服务。";
const SERVICE: Record<ServiceKind, { label: string; port: number; use: string }> = {
  embedding: { label: "向量服务", port: 8081, use: "挑选记忆、给资料建索引时" },
  rerank: { label: "重排服务", port: 8082, use: "给搜索结果重新排序时" },
};

function ServiceRow(props: { kind: ServiceKind; status: ServiceStatus }) {
  const toast = useToast();
  const refresh = useRefreshStatus();
  const { kind, status: s } = props;
  const meta = SERVICE[kind];
  const act = (a: "start" | "stop") => void youziApi.service(kind, a).then(refresh, (e) => toast(errorText(e), "error"));
  const choose = (key: string) => void youziApi.serviceModel(kind, key).then(refresh, (e) => toast(errorText(e), "error"));
  return (
    <>
      <Row
        label={meta.label}
        help={`127.0.0.1:${meta.port}，${meta.use}自动启动；柚子启动的闲置 30 分钟后自动停。模型从 ~/Documents/LLM 的模型清单里选${kind === "embedding" ? "，换了会在后台重算搜索用的向量" : ""}。`}
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
      await youziApi.keySave(props.service, value);
      setValue("");
      setEditing(false);
      void refresh();
      toast(`已保存 ${SERVICE_NAMES[props.service]} 的 Key`);
    } catch (err) {
      toast(errorText(err), "error");
    }
  };
  const remove = () => void youziApi.keyRemove(props.service).then(refresh, (e) => toast(errorText(e), "error"));
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

const EMPTY: CloudModel = { id: "", name: "", apiModel: "", service: "deepSeek", contextWindow: 131072, maxOutputTokens: 8192, thinking: "switch", efforts: [], defaultEffort: null };

function CloudModelDialog(props: { model: CloudModel; onClose: () => void; onSaved?: (m: CloudModel) => void }) {
  const [m, setM] = useState(props.model);
  const [error, setError] = useState<string | null>(null);
  const [leveled, setLeveled] = useState(props.model.efforts.length > 0);
  const [levels, setLevels] = useState(props.model.efforts.join(", "));
  const refresh = useRefreshStatus();
  const num = (v: string) => Number(v.replace(/[^\d]/g, "")) || 0;
  const words = leveled ? levels.split(/[\s,，、/]+/).filter(Boolean) : [];
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const saved = await youziApi.cloudModelSave({ ...m, efforts: words, defaultEffort: m.defaultEffort && words.includes(m.defaultEffort) ? m.defaultEffort : null });
      void refresh();
      props.onSaved?.(saved);
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
      {m.thinking !== "never" && (
        <Field label="思考强度">
          <div className="space-y-2">
            <label className="flex items-center gap-2 text-sm">
              <Switch label="支持思考强度" checked={leveled} onChange={setLeveled} /> 支持
            </label>
            {leveled && (
              <div className="flex items-center gap-2">
                <TextInput aria-label="强度选项" className="min-w-0 flex-1 font-mono" placeholder="low, high, max" value={levels} onChange={(e) => setLevels(e.target.value)} />
                <Dropdown<string | null> ariaLabel="默认强度" value={m.defaultEffort && words.includes(m.defaultEffort) ? m.defaultEffort : null} options={[{ value: null, label: "默认：不固定" }, ...words.map((w) => ({ value: w as string | null, label: `默认：${w}` }))]} onChange={(defaultEffort) => setM({ ...m, defaultEffort })} />
              </div>
            )}
          </div>
        </Field>
      )}
    </Modal>
  );
}

function CloudModels(props: { status: YouziStatus }) {
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
        {list.length === 0 ? <span className="text-muted-foreground">还没有</span> : <span className="block truncate">{byService(list).map(([sv, ms]) => `${SERVICE_NAMES[sv]} ${ms.length}`).join(" · ")}</span>}
      </Row>
      {open && <CloudModelsDialog models={list} onClose={() => setOpen(false)} />}
    </>
  );
}

const k = (n: number) => (n % 1024 === 0 ? `${n / 1024}K` : n.toLocaleString());

/** The models by provider, in the order the list first names each. */
function byService(models: CloudModel[]): [Service, CloudModel[]][] {
  return [...new Set(models.map((m) => m.service))].map((sv) => [sv, models.filter((m) => m.service === sv)]);
}

/** The cloud model list by provider (each folds; the open ones are remembered): drag a row to reorder within its provider (the model menu follows), edit, delete, add. */
function CloudModelsDialog(props: { models: CloudModel[]; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [editing, setEditing] = useState<CloudModel | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const toast = useToast();
  const refresh = useRefreshStatus();
  const [shown, setShown] = useSetting<string[]>("youzi.cloudGroups", [], isStringArray);
  const toggle = (sv: Service, open: boolean) => setShown(open ? [...shown.filter((x) => x !== sv), sv] : shown.filter((x) => x !== sv));
  // a provider's new order, its models keeping the places they had in the whole list
  const reorder = (group: CloudModel[], order: number[]) => {
    const next = order.map((i) => group[i]);
    const ids = props.models.map((m) => (m.service === group[0].service ? next.shift()!.id : m.id));
    void youziApi.cloudModelsReorder(ids).then(refresh, (e) => toast(errorText(e), "error"));
  };
  useEffect(() => {
    const d = ref.current;
    if (d && !d.open) d.showModal();
  }, []);
  const remove = async (m: CloudModel) => {
    if (!(await confirm({ title: `删除「${m.name}」？`, confirmLabel: "删除", danger: true }))) return;
    youziApi.cloudModelDelete(m.id).then(refresh, (e) => toast(errorText(e), "error"));
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
      <div className="-m-1 min-h-0 flex-auto overflow-y-auto p-1 px-3 py-2">
        {props.models.length === 0 && <p className="px-2 py-6 text-center text-sm text-muted-foreground">还没有</p>}
        {byService(props.models).map(([sv, ms]) => (
          <CloudGroup key={sv} service={sv} models={ms} open={shown.includes(sv)} onToggle={(open) => toggle(sv, open)} onReorder={(order) => reorder(ms, order)} onEdit={setEditing} onRemove={(m) => void remove(m)} />
        ))}
      </div>
      <div className="border-t p-2">
        <button type="button" onClick={() => setEditing(EMPTY)} className="flex items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted hover:text-foreground">
          <Plus size={14} /> 新增
        </button>
      </div>
      {editing && <CloudModelDialog model={editing} onClose={() => setEditing(null)} onSaved={(m) => toggle(m.service, true)} />}
      {confirmDialog}
    </dialog>
  );
}

/** One provider's models: its heading folds them; a row drags within the provider. */
function CloudGroup(props: { service: Service; models: CloudModel[]; open: boolean; onToggle: (open: boolean) => void; onReorder: (order: number[]) => void; onEdit: (m: CloudModel) => void; onRemove: (m: CloudModel) => void }) {
  const sort = useSortable(props.onReorder);
  return (
    <div>
      <button type="button" aria-expanded={props.open} onClick={() => props.onToggle(!props.open)} className="flex w-full items-center gap-1.5 rounded-control px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted/70">
        {props.open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <span className="text-foreground">{SERVICE_NAMES[props.service]}</span>
        <span className="text-xs">{props.models.length}</span>
      </button>
      {props.open && (
        <div ref={sort.listRef} className="pl-4">
          {props.models.map((m, i) => (
            <div key={m.id} {...sort.item(i)} className="rounded-control bg-popover">
              <div {...sort.handle(i)} className="group flex items-center gap-3 rounded-control px-2 py-2 text-sm hover:bg-muted/70">
                <GripVertical size={14} className="shrink-0 cursor-grab text-muted-foreground/60" />
                <span className="shrink-0">{m.name}</span>
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                  <span className="font-mono">{m.apiModel}</span> · 上下文 {k(m.contextWindow)} · 回复 {k(m.maxOutputTokens)} · {THINK_LABELS[m.thinking]}
                  {m.efforts.length > 0 && ` · ${m.efforts.join(" / ")}${m.defaultEffort ? `（默认 ${m.defaultEffort}）` : ""}`}
                </span>
                <span className="flex opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
                  <IconButton label="编辑" onClick={() => props.onEdit(m)}>
                    <Pencil size={14} strokeWidth={1.5} />
                  </IconButton>
                  <IconButton label="删除" danger onClick={() => props.onRemove(m)}>
                    <Trash2 size={14} strokeWidth={1.5} />
                  </IconButton>
                </span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const CLOUD_DATA_KEY = ["youzi", "data-cloud"];

/** Which modules' data may go to cloud models (the rest only to the local model). */
function CloudDataRow() {
  const qc = useQueryClient();
  const toast = useToast();
  const list = useQuery({ queryKey: CLOUD_DATA_KEY, queryFn: youziApi.dataCloud }).data;
  if (!list) return null;
  const toggle = (m: CloudData) =>
    void youziApi.dataCloudSet(m.module, !m.allowed).then(
      () => qc.invalidateQueries({ queryKey: CLOUD_DATA_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <Row label="发给云端" help="选云端模型时，柚子只查选中的模块，查到的内容会发给那家服务；没选中的只给本地模型。钱来钱往的收支和资产分开选，导入账单的 AI 跟着收支。芝麻开门和模块坞里隐藏的模块，柚子都不读。">
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
      help="从各模块的记录和柚子的对话算出来的搜索用的数据（按字找的索引和按意思找的向量；芝麻开门不算进去），单独存在数据目录的 search.db 里，不进备份和导出，删掉会自动重建。改过的记录，在按意思搜索时或向量服务开着时补算向量。"
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

const ASSIST_MODEL = ["youzi", "assistModel"];
const ASSIST_LEVEL = ["youzi", "assistLevel"];

/** 面板帮手 (润色, 一句话填好, 贴 JD…): its model; whether it thinks and how hard follow what that model can do. */
function AssistRow(props: { status: YouziStatus }) {
  const qc = useQueryClient();
  const toast = useToast();
  const { status } = props;
  const model = useQuery({ queryKey: ASSIST_MODEL, queryFn: youziApi.assistModel }).data;
  const stored = useQuery({ queryKey: ASSIST_LEVEL, queryFn: youziApi.assistLevel }).data;
  if (model === undefined || stored == null) return null;
  const setModel = (m: string | null) => void youziApi.assistModelSet(m).then((v) => qc.setQueryData(ASSIST_MODEL, v), (e) => toast(errorText(e), "error"));
  const setLevel = (l: Level) => void youziApi.assistLevelSet(l).then((v) => qc.setQueryData(ASSIST_LEVEL, v), (e) => toast(errorText(e), "error"));
  const code = model ?? status.selection;
  const level = levelFor(status, code, stored);
  const thinking = level !== "off";
  const { efforts, fixed } = levelsOf(status, code);
  const options: DropdownOption<string | null>[] = [{ value: null, label: "柚子现在选的" }, ...modelOptions(status)];
  if (model && !options.some((o) => o.value === model)) options.push({ value: model, label: "（已经不在了）", group: "" });
  const label = "text-xs text-muted-foreground";
  return (
    <Row label="面板帮手" help="润色、一句话填好、贴 JD、拆子任务、补全书影信息、打包清单都用这里的模型和思考。是否思考和思考强度跟着选的模型变。">
      <div className="flex flex-wrap items-center gap-x-8 gap-y-2.5">
        <div className="flex items-center gap-3">
          <span className={label}>模型选择</span>
          <Dropdown<string | null> ariaLabel="面板帮手的模型" foldKey="models" value={model} options={options} onChange={setModel} />
        </div>
        <div className="flex items-center gap-3">
          <span className={label}>是否思考</span>
          <Switch label="面板帮手是否思考" checked={thinking} disabled={thinkOf(status, code) !== "switch"} onChange={(on) => setLevel(on ? "on" : "off")} />
          {thinkOf(status, code) !== "switch" && <span className={label}>这个模型{THINK_LABELS[thinkOf(status, code)]}</span>}
        </div>
        <div className="flex items-center gap-3">
          <span className={label}>思考强度</span>
          {thinking && efforts.length > 0 ? (
            <Dropdown<Level> ariaLabel="面板帮手的思考强度" value={level} options={[{ value: "on", label: `默认${fixed ? `（${fixed}）` : ""}` }, ...efforts.map((e) => ({ value: e, label: e }))]} onChange={setLevel} />
          ) : (
            <span className={label}>{thinking ? "这个模型没有强度" : "不思考"}</span>
          )}
        </div>
      </div>
    </Row>
  );
}

export function YouziSection() {
  const status = useYouziStatus().data;
  const info = useQuery({ queryKey: ["youzi", "info"], queryFn: youziApi.info }).data;
  const memoryOn = useQuery({ queryKey: MEMORY_ENABLED_KEY, queryFn: youziApi.memoryEnabled });
  const qc = useQueryClient();
  const toast = useToast();
  if (!status) return null;
  const local = status.local;
  return (
    <Section title="柚子" help={HELP}>
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
                void youziApi.memoryEnabledSet(on).then(
                  () => qc.invalidateQueries({ queryKey: ["youzi"] }),
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
      <AssistRow status={status} />
      {info && (
        <Row label="数据库" help="柚子的对话和设置单独存在这个文件里，和生活数据分开备份、分开恢复。">
          <Path value={info.dbPath} />
          <span className="ml-2 text-xs text-muted-foreground">版本 {info.schemaVersion ?? "—"}</span>
        </Row>
      )}
      <StorageRow />
    </Section>
  );
}
