// 设置 → 小拾: model services (status, switch, stop), the embedding service, 使用记忆, 发给云端 (which modules'
// data a cloud model may get when 小拾 looks things up), cloud API keys (keychain
// only: shown as 已保存, never read back) and the cloud model list. Rules live in Rust (src-tauri/src/picky).
import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, Field, IconButton, Switch, TextInput } from "@/shared/ui";
import { MEMORY_ENABLED_KEY, SERVICE_NAMES, pickyApi, usePickyStatus, useRefreshStatus, type CloudData, type CloudModel, type PickyStatus, type Service } from "@/modules/picky/api";
import { IssueNotice, ModelMenu, StatusDot } from "@/modules/picky/ModelMenu";
import { Path, Row, Section } from "./layout";
import { StorageRow } from "./PickyStorage";

const HELP =
  "本地模型和向量服务由 ~/Documents/LLM 里的脚本管理，小拾只启动、切换和停止它们，不改里面的文件；同一时间只运行一个本地模型，退出柚儿园后模型继续运行。选云端模型时，这一轮的消息会发给那家服务。";
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

const EMPTY: CloudModel = { id: "", name: "", apiModel: "", service: "deepSeek", contextWindow: 131072, maxOutputTokens: 8192 };

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
    </Modal>
  );
}

function CloudModels(props: { status: PickyStatus }) {
  const [editing, setEditing] = useState<CloudModel | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const toast = useToast();
  const refresh = useRefreshStatus();
  const remove = async (m: CloudModel) => {
    if (!(await confirm({ title: `删除「${m.name}」？`, confirmLabel: "删除", danger: true }))) return;
    pickyApi.cloudModelDelete(m.id).then(refresh, (e) => toast(errorText(e), "error"));
  };
  const k = (n: number) => (n % 1024 === 0 ? `${n / 1024}K` : n.toLocaleString());
  return (
    <>
      <Row label="云端模型" actions={<Button onClick={() => setEditing(EMPTY)}><Plus size={14} strokeWidth={1.5} /> 新增</Button>}>
        {props.status.cloud.length === 0 && <span className="text-muted-foreground">还没有</span>}
      </Row>
      {props.status.cloud.map((m) => (
        <div key={m.id} className="group flex items-center gap-4 border-b py-2 pr-4 pl-[152px] last:border-b-0">
          <span className="min-w-0 truncate">{m.name}</span>
          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
            {SERVICE_NAMES[m.service]} · <span className="font-mono">{m.apiModel}</span> · 上下文 {k(m.contextWindow)} · 回复 {k(m.maxOutputTokens)}
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
      ))}
      {editing && <CloudModelDialog model={editing} onClose={() => setEditing(null)} />}
      {confirmDialog}
    </>
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

export function PickySection() {
  const status = usePickyStatus().data;
  const info = useQuery({ queryKey: ["picky", "info"], queryFn: pickyApi.info }).data;
  const memoryOn = useQuery({ queryKey: MEMORY_ENABLED_KEY, queryFn: pickyApi.memoryEnabled });
  const qc = useQueryClient();
  const toast = useToast();
  const refresh = useRefreshStatus();
  if (!status) return null;
  const local = status.local;
  const emb = status.embedding;
  const embAction = (a: "start" | "stop") => void pickyApi.embedding(a).then(refresh, (e) => toast(errorText(e), "error"));
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
      <Row
        label="向量服务"
        help="Qwen3-Embedding-0.6B（127.0.0.1:8081），挑选记忆、给资料建索引时会自动启动。"
        actions={
          emb.state === "stopped" ? (
            <Button disabled={emb.working} onClick={() => embAction("start")}>启动</Button>
          ) : (
            <Button disabled={emb.working} onClick={() => embAction("stop")}>停止</Button>
          )
        }
      >
        {emb.text}
      </Row>
      {emb.issue && (
        <div className="border-b px-4 py-3">
          <IssueNotice issue={emb.issue} compact />
        </div>
      )}
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
