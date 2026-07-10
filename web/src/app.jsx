import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { langFor, highlightLine } from './hl.js';

export function App() {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [split, setSplit] = useState(true);
  const [theme, setTheme] = useState(
    () =>
      localStorage.getItem('skimdiff-theme') ||
      (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
  );
  const [live, setLive] = useState(false);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('skimdiff-theme', theme);
  }, [theme]);

  const [review, setReview] = useState({});

  const load = () =>
    fetch('/api/diff')
      .then((r) => r.json())
      .then((d) => {
        if (d.error) throw new Error(d.error);
        setData(d);
        setError(null);
      })
      .catch((e) => setError(String(e)));

  const loadReview = () =>
    fetch('/api/state')
      .then((r) => r.json())
      .then((s) => setReview(s.files ?? {}));

  const postReview = (path, patch) =>
    fetch('/api/state', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, ...patch }),
    })
      .then((r) => r.json())
      .then((s) => setReview(s.files ?? {}));

  const hunkAction = (path, hunk, action) => {
    if (action === 'discard' && !confirm(`Discard this hunk in ${path}? This rewrites the file on disk.`)) return;
    fetch('/api/hunk', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path, hunk, action }),
    })
      .then((r) => r.json())
      .then((res) => {
        if (res.error) alert(res.error);
        load();
      });
  };

  const [usages, setUsages] = useState(null); // { name, declarations, references }
  const [viewer, setViewer] = useState(null); // { path, line }

  const openUsages = (name) =>
    fetch(`/api/usages?name=${encodeURIComponent(name)}`)
      .then((r) => r.json())
      .then((u) => setUsages({ name, ...u }));

  useEffect(() => {
    load();
    loadReview();
  }, []);

  // double-click any identifier to look it up
  useEffect(() => {
    const onDbl = (e) => {
      if (!e.target.closest('.code, .fileviewer')) return;
      const word = (getSelection()?.toString() ?? '').trim();
      if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(word)) openUsages(word);
    };
    document.addEventListener('dblclick', onDbl);
    return () => document.removeEventListener('dblclick', onDbl);
  }, []);

  useEffect(() => {
    const onEsc = (e) => {
      if (e.key !== 'Escape') return;
      if (viewer) setViewer(null);
      else setUsages(null);
    };
    window.addEventListener('keydown', onEsc);
    return () => window.removeEventListener('keydown', onEsc);
  }, [viewer]);

  // live mode: refresh when the server reports working-tree changes
  useEffect(() => {
    if (!data || data.mode !== 'live') return;
    const es = new EventSource('/api/events');
    es.onopen = () => setLive(true);
    es.onerror = () => setLive(false);
    es.onmessage = () => load();
    return () => es.close();
  }, [data?.mode]);

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
      if (e.key === 'v' && current)
        postReview(current.path, { viewed: !review[current.path]?.viewed });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [files, current, review]);

  if (error) return <div class="msg err">{error}</div>;
  if (!data) return <div class="msg">loading…</div>;

  return (
    <div class="layout">
      <aside class="sidebar">
        <header>
          <span class="logo">skimdiff</span>
          <span class="mode">
            {data.mode === 'live' ? 'working tree' : data.range}
            {data.mode === 'live' && <span class={`dot ${live ? 'on' : ''}`} title={live ? 'live' : 'disconnected'} />}
          </span>
          <button
            class="themebtn"
            title="toggle theme"
            onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
          >
            {theme === 'dark' ? '☀' : '☾'}
          </button>
        </header>
        <ul class="filelist">
          {files.map((f) => (
            <FileItem
              key={f.path}
              f={f}
              active={current && f.path === current.path}
              viewed={review[f.path]?.viewed}
              onClick={() => setSelected(f.path)}
            />
          ))}
          {files.length === 0 && <li class="empty">no changes</li>}
        </ul>
        <footer class="hints">n/p file · j/k hunk · v viewed · u split · 2×click symbol</footer>
      </aside>
      <main class="content">
        {current ? (
          <FileDiff
            f={current}
            split={split}
            live={data.mode === 'live'}
            rev={review[current.path]}
            onReview={(patch) => postReview(current.path, patch)}
            onHunk={(i, action) => hunkAction(current.path, i, action)}
          />
        ) : (
          <div class="msg">nothing to review — working tree is clean</div>
        )}
      </main>
      {usages && (
        <UsagesPanel
          u={usages}
          onClose={() => setUsages(null)}
          onOpen={(path, line) => setViewer({ path, line })}
        />
      )}
      {viewer && <FileViewer path={viewer.path} line={viewer.line} onClose={() => setViewer(null)} />}
    </div>
  );
}

function UsagesPanel({ u, onClose, onOpen }) {
  const section = (title, items) => (
    <div class="usec">
      <div class="utitle">{title} ({items.length})</div>
      {items.map((s, i) => (
        <div class="uitem" key={i} onClick={() => onOpen(s.path, s.line)}>
          <span class="uloc">{s.path}:{s.line}</span>
          <code class="uctx">{s.context}</code>
        </div>
      ))}
      {items.length === 0 && <div class="uempty">none</div>}
    </div>
  );
  return (
    <aside class="usages">
      <header>
        <code class="uname">{u.name}</code>
        <button class="themebtn" onClick={onClose}>✕</button>
      </header>
      {section('Declarations', u.declarations)}
      {section('Usages', u.references)}
    </aside>
  );
}

function FileViewer({ path, line, onClose }) {
  const [content, setContent] = useState(null);
  const lang = langFor(path);
  useEffect(() => {
    setContent(null);
    fetch(`/api/file?path=${encodeURIComponent(path)}`)
      .then((r) => r.json())
      .then((f) => setContent(f.error ? `error: ${f.error}` : f.content));
  }, [path]);
  useEffect(() => {
    if (content != null)
      document.querySelector('.fileviewer .target')?.scrollIntoView({ block: 'center' });
  }, [content]);
  return (
    <div class="overlay" onClick={(e) => e.target.classList.contains('overlay') && onClose()}>
      <div class="fileviewer">
        <header>
          <span class="path">{path}</span>
          <button class="themebtn" onClick={onClose}>✕</button>
        </header>
        <div class="fvbody">
          {content == null ? (
            <div class="msg">loading…</div>
          ) : (
            <table class="difftable">
              {content.split('\n').map((l, i) => {
                const html = highlightLine(l, lang);
                return (
                  <tr key={i} class={i + 1 === line ? 'target' : ''}>
                    <td class="no">{i + 1}</td>
                    <td class="code">
                      {html !== null ? <span dangerouslySetInnerHTML={{ __html: html }} /> : <span>{l}</span>}
                    </td>
                  </tr>
                );
              })}
            </table>
          )}
        </div>
      </div>
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

function FileItem({ f, active, viewed, onClick }) {
  const { add, del } = stats(f);
  const dir = f.path.includes('/') ? f.path.slice(0, f.path.lastIndexOf('/') + 1) : '';
  const base = f.path.slice(dir.length);
  return (
    <li class={`${active ? 'active' : ''} ${viewed ? 'viewed' : ''}`} onClick={onClick} title={f.path}>
      <span class={`badge ${f.status}`}>{f.status[0].toUpperCase()}</span>
      <span class="fname">
        {dir && <span class="dir">{dir}</span>}
        {base}
      </span>
      <span class="counts">
        {viewed && <span class="check">✓</span>}
        {add > 0 && <span class="plus">+{add}</span>}
        {del > 0 && <span class="minus">−{del}</span>}
      </span>
    </li>
  );
}

function FileDiff({ f, split, live, rev, onReview, onHunk }) {
  const lang = langFor(f.path);
  return (
    <div class="filediff" data-path={f.path}>
      <div class="fileheader">
        <span class={`badge ${f.status}`}>{f.status}</span>
        <span class="path">
          {f.old_path ? `${f.old_path} → ${f.path}` : f.path}
        </span>
        <label class="viewedbox">
          <input
            type="checkbox"
            checked={!!rev?.viewed}
            onChange={(e) => onReview({ viewed: e.currentTarget.checked })}
          />
          viewed
        </label>
      </div>
      <NoteBox note={rev?.note ?? ''} onSave={(note) => onReview({ note })} />
      {f.is_binary && <div class="msg">binary file</div>}
      {f.hunks.map((h, i) => (
        <div class="hunk" key={i}>
          <div class="hunkheader">
            <span>
              @@ −{h.old_start},{h.old_lines} +{h.new_start},{h.new_lines} @@ {h.header}
            </span>
            {live && (
              <span class="hunkactions">
                <button onClick={() => onHunk(i, 'stage')} title="git add this hunk">stage</button>
                <button class="danger" onClick={() => onHunk(i, 'discard')} title="revert this hunk on disk">discard</button>
              </span>
            )}
          </div>
          {split ? <SplitHunk h={h} lang={lang} /> : <UnifiedHunk h={h} lang={lang} />}
        </div>
      ))}
    </div>
  );
}

function NoteBox({ note, onSave }) {
  const [val, setVal] = useState(note);
  useEffect(() => setVal(note), [note]);
  return (
    <div class="notebox">
      <input
        type="text"
        placeholder="review note…"
        value={val}
        onInput={(e) => setVal(e.currentTarget.value)}
        onBlur={() => val !== note && onSave(val)}
        onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
      />
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
