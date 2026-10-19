// 开机自动启动 (rules in Rust autostart.rs): only the installed app writes the login item; other copies
// keep the setting.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { api } from "@/shared/ipc";
import { Switch } from "@/shared/ui";
import { Row, Section } from "./layout";

const KEY = ["app", "autostart"];

export function StartupSection() {
  const qc = useQueryClient();
  const toast = useToast();
  const { data } = useQuery({ queryKey: KEY, queryFn: api.autostart });
  if (!data) return null;
  const set = (on: boolean) =>
    void api.autostartSet(on).then(
      (v) => qc.setQueryData(KEY, v),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <Section title="启动">
      <Row label="开机自动启动" help={data.registers ? "登录 Mac 时打开柚儿园，只在菜单栏，不弹窗口。" : "登录 Mac 时打开柚儿园，只在菜单栏，不弹窗口。这里只记下设置，正式版才会登记。"}>
        <Switch label="开机自动启动" checked={data.on} onChange={set} />
      </Row>
    </Section>
  );
}
