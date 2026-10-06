// 芝麻开门 before it opens: set the master password (first time) or unlock (Touch ID when Rust allows
// it — started by itself only when arriving at the page, not after locking there — else the master password).
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Fingerprint, KeyRound } from "lucide-react";
import { errorText } from "@/shared/feedback";
import { Button, TextInput } from "@/shared/ui";
import { REASON, VAULT_KEY, VAULT_STATUS_KEY, vaultApi, type VaultStatus } from "./api";

function Card(props: { color: string; children: ReactNode }) {
  return (
    <div className="flex flex-1 items-center justify-center pb-16">
      <div className="w-80 space-y-4 text-center">
        <KeyRound size={36} strokeWidth={1.5} className="mx-auto" style={{ color: props.color }} />
        {props.children}
      </div>
    </div>
  );
}

const pw = "w-full text-center";

function useSettle() {
  const qc = useQueryClient();
  return (s: VaultStatus) => {
    qc.setQueryData(VAULT_STATUS_KEY, s);
    void qc.invalidateQueries({ queryKey: VAULT_KEY, predicate: (q) => q.queryKey[1] !== "status" });
  };
}

export function Setup(props: { color: string }) {
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const settle = useSettle();
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    if (a !== b) return setError("两次输入的不一样");
    setBusy(true);
    try {
      settle(await vaultApi.setup(a));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card color={props.color}>
      <p className="text-lg font-bold">设置主密码</p>
      <form onSubmit={submit} className="space-y-2">
        <TextInput type="password" autoFocus autoComplete="new-password" aria-label="主密码" placeholder="主密码" value={a} onChange={(e) => setA(e.target.value)} className={pw} />
        <TextInput type="password" autoComplete="new-password" aria-label="再输一次" placeholder="再输一次" value={b} onChange={(e) => setB(e.target.value)} className={pw} />
        {error && <p className="text-sm text-destructive">{error}</p>}
        <Button type="submit" variant="primary" disabled={!a || !b || busy} className="w-full justify-center">
          设置
        </Button>
      </form>
      <p className="text-xs leading-relaxed text-balance text-muted-foreground">主密码只记在你心里，Pomelo 不保存它。忘了就再也打不开。</p>
    </Card>
  );
}

export function Unlock(props: { color: string; status: VaultStatus; autoTouch: boolean }) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"touch" | "password" | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const settle = useSettle();
  const qc = useQueryClient();
  const tried = useRef(false);

  const touch = async () => {
    setError(null);
    setBusy("touch");
    try {
      const s = await vaultApi.unlockTouch();
      settle(s);
      if (s.state !== "open") input.current?.focus(); // cancelled / chose 输入主密码
    } catch (err) {
      setError(errorText(err));
      void qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY });
      input.current?.focus();
    } finally {
      setBusy(null);
    }
  };
  // Touch ID straight away on arrival
  useEffect(() => {
    if (props.autoTouch && props.status.touch && !tried.current) {
      tried.current = true;
      void touch();
    }
  }, [props.status.touch]); // eslint-disable-line react-hooks/exhaustive-deps

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!password || busy) return;
    setError(null);
    setBusy("password");
    try {
      settle(await vaultApi.unlock(password));
    } catch (err) {
      setError(errorText(err));
      setPassword("");
      input.current?.focus();
    } finally {
      setBusy(null);
    }
  };
  const reason = props.status.reason ? REASON[props.status.reason] : null;
  return (
    <Card color={props.color}>
      {reason && !error && <p className="text-sm text-muted-foreground">{reason}</p>}
      {props.status.touch && (
        <Button onClick={() => void touch()} disabled={busy !== null} className="w-full justify-center">
          <Fingerprint size={16} /> 用 Touch ID 打开
        </Button>
      )}
      <form onSubmit={submit} className="space-y-2">
        <TextInput
          ref={input}
          type="password"
          autoFocus={!props.status.touch}
          autoComplete="current-password"
          aria-label="主密码"
          placeholder="主密码"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={pw}
        />
        {error && <p className="text-sm text-destructive">{error}</p>}
        <Button type="submit" variant="primary" disabled={!password || busy !== null} className="w-full justify-center">
          {busy === "password" ? "正在打开…" : "打开"}
        </Button>
      </form>
    </Card>
  );
}
