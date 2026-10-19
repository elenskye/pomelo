// 新建 / 编辑项目: a name, the background instructions its conversations send, whether they also use
// global memories, and the 知识库 they search (none chosen = all).
import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorText } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Field, Switch, TextArea, TextInput } from "@/shared/ui";
import { INSTRUCTIONS_KEY, LIBRARY_KEY, pickyApi, type ProjectRow } from "./api";
import { ProjectLibraries } from "./Knowledge";

export function ProjectDialog(props: { project: ProjectRow | null; onClose: () => void; onCreated?: (id: number) => void }) {
  const [name, setName] = useState(props.project?.name ?? "");
  const [instructions, setInstructions] = useState(props.project?.instructions ?? "");
  const [globalMemories, setGlobalMemories] = useState(props.project?.includeGlobalMemories ?? true);
  const [libraries, setLibraries] = useState(props.project?.libraries ?? []);
  const [error, setError] = useState<string | null>(null);
  const qc = useQueryClient();
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      if (props.project) await pickyApi.projectUpdate(props.project.id, name, instructions, globalMemories, libraries);
      else {
        const id = await pickyApi.projectCreate(name, instructions, globalMemories, libraries);
        props.onCreated?.(id);
      }
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
      void qc.invalidateQueries({ queryKey: INSTRUCTIONS_KEY });
      props.onClose();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title={props.project ? "项目设置" : "新建项目"} onClose={props.onClose} onSubmit={submit} error={error} submitLabel={props.project ? "保存" : "新建"} wide>
      <Field label="名称">
        <TextInput autoFocus value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label="背景指令">
        <TextArea rows={6} value={instructions} onChange={(e) => setInstructions(e.target.value)} className="font-reading" />
      </Field>
      <span className="flex items-center gap-2 text-sm">
        同时使用全局记忆 <Switch label="同时使用全局记忆" checked={globalMemories} onChange={setGlobalMemories} />
      </span>
      <ProjectLibraries value={libraries} onChange={setLibraries} />
    </Modal>
  );
}
