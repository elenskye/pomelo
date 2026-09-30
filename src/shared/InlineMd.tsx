// Renders the one bit of Markdown titles and notes use: **bold**. Everything else stays plain text.
export function InlineMd(props: { text: string }) {
  const parts = props.text.split(/(\*\*[^*\n]+\*\*)/);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("**") && p.endsWith("**") && p.length > 4 ? (
          <b key={i} className="font-bold">
            {p.slice(2, -2)}
          </b>
        ) : (
          p
        ),
      )}
    </>
  );
}
