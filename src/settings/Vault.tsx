// 设置 → 芝麻开门: the rules in one (?) and 修改主密码 (needs the current one; Rust checks it).
import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, Field, TextInput } from "@/shared/ui";
import { VAULT_KEY, VAULT_STATUS_KEY, useVaultStatus, vaultApi } from "@/modules/vault/api";
import { Row, Section } from "./layout";

const HELP =
  "平时用 Touch ID 打开。指纹框里 3 次都没对上、距上次输主密码超过 3 天、或者 App 更新后，就要输一次主密码。闲置 5 分钟自动锁上。复制的内容 30 秒后从剪贴板清掉。密码加密后才存进数据库，备份里也是加密的；导出文件里没有密码。主密码只有你知道，忘了就打不开，也没法重设。";

function ChangeDialog(props: { onClose: () => void }) {
  const [old, setOld] = useState("");
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const qc = useQueryClient();
  const toast = useToast();
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setError(null);
    if (a !== b) return setError("两次输入的新主密码不一样");
    setBusy(true);
    try {
      qc.setQueryData(VAULT_STATUS_KEY, await vaultApi.changePassword({ old, new: a }));
      void qc.invalidateQueries({ queryKey: VAULT_KEY });
      toast("主密码已修改");
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="修改主密码" onClose={props.onClose} onSubmit={submit} error={error} submitLabel={busy ? "正在修改…" : "修改"}>
      <Field label="原来的主密码">
        <TextInput type="password" autoFocus autoComplete="current-password" value={old} onChange={(e) => setOld(e.target.value)} />
      </Field>
      <Field label="新主密码">
        <TextInput type="password" autoComplete="new-password" value={a} onChange={(e) => setA(e.target.value)} />
      </Field>
      <Field label="再输一次">
        <TextInput type="password" autoComplete="new-password" value={b} onChange={(e) => setB(e.target.value)} />
      </Field>
    </Modal>
  );
}

export function VaultSection() {
  const status = useVaultStatus().data;
  const [changing, setChanging] = useState(false);
  if (!status) return null;
  const set = status.state !== "new";
  return (
    <Section title="芝麻开门">
      <Row label="主密码" help={HELP} actions={set ? <Button onClick={() => setChanging(true)}>修改…</Button> : undefined}>
        {set ? "已设置" : "还没设置"}
      </Row>
      {changing && <ChangeDialog onClose={() => setChanging(false)} />}
    </Section>
  );
}
