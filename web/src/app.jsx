import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { langFor, highlightLine } from './hl.js';

export function App() {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [split, setSplit] = useState(true);

  const load = () =>
    fetch('/api/diff')
      .then((r) => r.json())
      .then((d) => {
        if (d.error) throw new Error(d.error);
        setData(d);
        setError(null);
      })
      .catch((e) => setError(String(e)));

  useEffect(() => {
    load();
  }, []);

  const files = data?.files ?? [];
  const current = files.find((f) => f.path === selected) ?? files[0] ?? null;

  useEffect(() => {
    const onKey = (e) => {
      if (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA') return;
      const idx = files.findIndex((f) => f.path === (current && current.path));
      if (e.key === 'n' && idx < files.length - 1) setSelected(files[idx + 1].path);
      if (e.key === 'p' && idx > 0) setSelected(files[idx - 1].path);
      if (e.key === 'u') setSplit((s) => !s);
      if (e.key === 'j' || e.key === 'k') jumpHunk(e.key === 'j' ? 1 : -1);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [files, current]);

  if (error) return <div class="msg err">{error}</div>;
  if (!data) return <div class="msg">loading…</div>;

  return (
    <div class="layout">
      <aside class="sidebar">
        <header>
          <span class="logo">skimdiff</span>
          <span class="mode">{data.mode === 'live' ? 'working tree' : data.range}</span>
        </header>
        <ul class="filelist">
          {files.map((f) => (
            <FileItem key={f.path} f={f} active={current && f.path === current.path} onClick={() => setSelected(f.path)} />
          ))}
          {files.length === 0 && <li class="empty">no changes</li>}
        </ul>
        <footer class="hints">n/p file · j/k hunk · u split</footer>
      </aside>
      <main class="content">
        {current ? (
          <FileDiff f={current} split={split} />
        ) : (
          <div class="msg">nothing to review — working tree is clean</div>
        )}
      </main>
    </div>
  );
}

function stats(f) {
  let add = 0, del = 0;
  for (const h of f.hunks) for (const l of h.lines) {
    if (l.kind === 'add') add++;
    else if (l.kind === 'del') del++;
  }
  return { add, del };
}

function FileItem({ f, active, onClick }) {
  const { add, del } = stats(f);
  const dir = f.path.includes('/') ? f.path.slice(0, f.path.lastIndexOf('/') + 1) : '';
  const base = f.path.slice(dir.length);
  return (
    <li class={active ? 'active' : ''} onClick={onClick} title={f.path}>
      <span class={`badge ${f.status}`}>{f.status[0].toUpperCase()}</span>
      <span class="fname">
        {dir && <span class="dir">{dir}</span>}
        {base}
      </span>
      <span class="counts">
        {add > 0 && <span class="plus">+{add}</span>}
        {del > 0 && <span class="minus">−{del}</span>}
      </span>
    </li>
  );
}

function FileDiff({ f, split }) {
  const lang = langFor(f.path);
  return (
    <div class="filediff" data-path={f.path}>
      <div class="fileheader">
        <span class={`badge ${f.status}`}>{f.status}</span>
        <span class="path">
          {f.old_path ? `${f.old_path} → ${f.path}` : f.path}
        </span>
      </div>
      {f.is_binary && <div class="msg">binary file</div>}
      {f.hunks.map((h, i) => (
        <div class="hunk" key={i}>
          <div class="hunkheader">
            @@ −{h.old_start},{h.old_lines} +{h.new_start},{h.new_lines} @@ {h.header}
          </div>
          {split ? <SplitHunk h={h} lang={lang} /> : <UnifiedHunk h={h} lang={lang} />}
        </div>
      ))}
    </div>
  );
}

function Code({ line, lang }) {
  if (line.spans) {
    return (
      <span>
        {line.spans.map((s, i) => (
          <span key={i} class={s.changed ? 'chg' : ''}>{s.text}</span>
        ))}
      </span>
    );
  }
  const html = highlightLine(line.content, lang);
  if (html !== null) return <span dangerouslySetInnerHTML={{ __html: html }} />;
  return <span>{line.content}</span>;
}

function UnifiedHunk({ h, lang }) {
  return (
    <table class="difftable unified">
      {h.lines.map((l, i) => (
        <tr key={i} class={l.kind}>
          <td class="no">{l.old_no ?? ''}</td>
          <td class="no">{l.new_no ?? ''}</td>
          <td class="sign">{l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ''}</td>
          <td class="code"><Code line={l} lang={lang} /></td>
        </tr>
      ))}
    </table>
  );
}

// Pair del-runs with add-runs into side-by-side rows.
function splitRows(lines) {
  const rows = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.kind === 'context') {
      rows.push({ left: l, right: l });
      i++;
    } else if (l.kind === 'del') {
      const dels = [];
      while (i < lines.length && lines[i].kind === 'del') dels.push(lines[i++]);
      const adds = [];
      while (i < lines.length && lines[i].kind === 'add') adds.push(lines[i++]);
      const n = Math.max(dels.length, adds.length);
      for (let k = 0; k < n; k++) rows.push({ left: dels[k] ?? null, right: adds[k] ?? null });
    } else {
      rows.push({ left: null, right: l });
      i++;
    }
  }
  return rows;
}

function SplitHunk({ h, lang }) {
  const rows = useMemo(() => splitRows(h.lines), [h]);
  return (
    <table class="difftable split">
      {rows.map((r, i) => (
        <tr key={i}>
          <td class="no">{r.left?.old_no ?? ''}</td>
          <td class={`code half ${r.left ? (r.left.kind === 'del' ? 'del' : '') : 'void'}`}>
            {r.left && <Code line={r.left} lang={lang} />}
          </td>
          <td class="no">{r.right?.new_no ?? ''}</td>
          <td class={`code half ${r.right ? (r.right.kind === 'add' ? 'add' : '') : 'void'}`}>
            {r.right && <Code line={r.right} lang={lang} />}
          </td>
        </tr>
      ))}
    </table>
  );
}

function jumpHunk(dir) {
  const hunks = [...document.querySelectorAll('.hunk')];
  if (!hunks.length) return;
  const y = window.scrollY + 60;
  let target;
  if (dir > 0) target = hunks.find((h) => h.offsetTop > y + 5);
  else target = [...hunks].reverse().find((h) => h.offsetTop < y - 5);
  if (target) window.scrollTo({ top: target.offsetTop - 50, behavior: 'smooth' });
}
