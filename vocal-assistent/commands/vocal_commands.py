#!/usr/bin/env python3
"""VOCAL-ASSISTENT voice commands ("Computador, ...").

Handy calls:  python3 -I vocal_commands.py "<command text>"

The local LLM (Ollama) only CHOOSES one action from the whitelist below and
fills its parameters as JSON. This script validates the choice and runs it with
fixed code. No free-form shell command from the model is ever executed.

Add a new action: write a function `act_<name>(params) -> str`, add it to
ACTIONS, and describe it in ACTION_HELP.
"""

import datetime
import difflib
import glob
import json
import os
import re
import subprocess
import sys
import unicodedata
import urllib.parse
import urllib.request

OLLAMA_URL = "http://localhost:11434/api/chat"
MODEL = os.environ.get("VOCAL_COMMANDS_MODEL", "gemma4:e4b")
LOG = os.path.expanduser("~/Library/Logs/com.pais.handy/vocal-commands.log")
PROJECT_ROOTS = ["~/Desktop", "~/Documents", "~/Developer", "~/Projects", "~/Music", "~/Downloads"]

ACTION_HELP = """
- open_app {"app"}: open an application. Ex: "abre o Slack", "open Spotify"
- quit_app {"app"}: quit an application. Ex: "fecha o Spotify"
- open_project {"name", "app"}: open a project folder or file by name, optionally in an app
    (VS Code, Cursor, REAPER, Finder...). Ex: "abre o projeto vocal assistent no VS Code",
    "abre o projeto Cliente X no Reaper"
- slack_open {"conversation"}: open a Slack channel, group or person. Ex: "abre o Slack no grupo marketing"
- chrome_close_tabs {"which"}: which = "all" | "current" | "others". Ex: "fecha as abas do Chrome" -> all,
    "fecha essa aba" -> current, "fecha as outras abas" -> others
- whatsapp_open {"conversation"}: open a WhatsApp chat. Ex: "abre o WhatsApp na conversa com a Maria"
- send_message {"app", "conversation", "message"}: write a message in Slack or WhatsApp.
    app = "slack" | "whatsapp". Ex: "manda no WhatsApp pra Maria: chego em 10 minutos",
    "escreve no Slack no grupo marketing que o post está pronto"
- spotify {"command", "query"}: command = "play" | "pause" | "next" | "previous" | "search".
    Ex: "pausa a música" -> pause, "próxima música" -> next, "toca Coldplay no Spotify" -> search + query
- volume {"level"}: level = a number 0-100, or "up", "down", "mute", "unmute".
    Ex: "volume 30", "aumenta o volume" -> up, "tira o som" -> mute
- window {"app", "position"}: position = "left" | "right" | "full" | "center" | "top" | "bottom".
    Ex: "coloca o Chrome na esquerda", "maximiza o VS Code" -> full, "janela atual na direita" -> right (no app)
- open_url {"url"}: open a website. Ex: "abre o YouTube" -> https://www.youtube.com
- web_search {"query"}: search Google. Ex: "pesquisa previsão do tempo em São Paulo"
- unknown {}: anything else, or if you are not sure
""".strip()

SCHEMA = {
    "type": "object",
    "properties": {
        "action": {
            "type": "string",
            "enum": ["open_app", "quit_app", "open_project", "slack_open", "whatsapp_open",
                     "send_message", "spotify", "volume", "window",
                     "chrome_close_tabs", "open_url", "web_search", "unknown"],
        },
        "app": {"type": "string"},
        "name": {"type": "string"},
        "conversation": {"type": "string"},
        "which": {"type": "string", "enum": ["all", "current", "others"]},
        "url": {"type": "string"},
        "query": {"type": "string"},
        "message": {"type": "string"},
        "command": {"type": "string", "enum": ["play", "pause", "next", "previous", "search"]},
        "level": {"type": "string"},
        "position": {"type": "string", "enum": ["left", "right", "full", "center", "top", "bottom"]},
    },
    "required": ["action"],
}


# ------------------------------------------------------------------ helpers
def log(msg):
    os.makedirs(os.path.dirname(LOG), exist_ok=True)
    with open(LOG, "a") as f:
        f.write(f"[{datetime.datetime.now():%Y-%m-%d %H:%M:%S}] {msg}\n")


def notify(msg):
    script = f'display notification {json.dumps(msg)} with title "VOCAL-ASSISTENT"'
    subprocess.run(["osascript", "-e", script], capture_output=True)


def osa(script):
    r = subprocess.run(["osascript", "-e", script], capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(r.stderr.strip() or "AppleScript failed")
    return r.stdout.strip()


def norm(s):
    s = unicodedata.normalize("NFKD", s or "").encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", " ", s.lower()).strip()


def installed_apps():
    apps = {}
    for root in ["/Applications", "/Applications/Utilities", "/System/Applications",
                 "/System/Applications/Utilities", os.path.expanduser("~/Applications")]:
        for path in glob.glob(os.path.join(root, "*.app")):
            apps[os.path.basename(path)[:-4]] = path
    return apps


ALIASES = {"vs code": "Visual Studio Code", "vscode": "Visual Studio Code", "code": "Visual Studio Code",
           "chrome": "Google Chrome", "whats": "WhatsApp", "zap": "WhatsApp", "word": "Microsoft Word",
           "excel": "Microsoft Excel", "terminal": "Terminal", "finder": "Finder"}


NOT_AN_APP = {"current", "atual", "essa", "esta", "this", "front", "frontmost", "janela", "window", "none", ""}


def find_app(name):
    """Fuzzy-match an app name against what is installed. Returns (name, path) or None."""
    apps = installed_apps()
    if norm(name) in NOT_AN_APP:
        return None
    wanted = ALIASES.get(norm(name), name)
    by_norm = {norm(a): a for a in apps}
    n = norm(wanted)
    if n in by_norm:
        return by_norm[n], apps[by_norm[n]]
    contains = [a for k, a in by_norm.items() if n and (n in k or k in n)]
    if contains:
        best = min(contains, key=len)
        return best, apps[best]
    close = difflib.get_close_matches(n, list(by_norm), n=1, cutoff=0.6)
    if close:
        return by_norm[close[0]], apps[by_norm[close[0]]]
    return None


def find_project(name, want_file_ext=None):
    """Most recently modified folder/file under PROJECT_ROOTS whose name matches."""
    n = norm(name)
    if not n:
        return None
    candidates = []
    for root in PROJECT_ROOTS:
        root = os.path.expanduser(root)
        if not os.path.isdir(root):
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            depth = dirpath[len(root):].count(os.sep)
            dirnames[:] = [d for d in dirnames if not d.startswith(".") and d not in
                           ("node_modules", "target", "dist", "build", "Library")]
            if depth >= 3:
                dirnames[:] = []
            entries = filenames if want_file_ext else dirnames
            for e in entries:
                if want_file_ext and not e.lower().endswith(want_file_ext):
                    continue
                stem = os.path.splitext(e)[0] if want_file_ext else e
                k = norm(stem)
                if k and (n in k or k in n or difflib.SequenceMatcher(None, n, k).ratio() > 0.75):
                    p = os.path.join(dirpath, e)
                    is_repo = os.path.isdir(os.path.join(p, ".git"))
                    # Best: exact name, then a project root (git repo), then shallower, then newest.
                    candidates.append(((k == n, is_repo, -depth, os.path.getmtime(p)), p))
    return max(candidates)[1] if candidates else None


# ------------------------------------------------------------------ actions
def act_open_app(p):
    found = find_app(p.get("app", ""))
    if not found:
        return f"Não achei o app \"{p.get('app', '')}\""
    subprocess.run(["open", "-a", found[1]], check=True)
    return f"Abrindo {found[0]}"


def act_quit_app(p):
    found = find_app(p.get("app", ""))
    if not found:
        return f"Não achei o app \"{p.get('app', '')}\""
    osa(f'tell application {json.dumps(found[0])} to quit')
    return f"Fechando {found[0]}"


def app_in_text(text):
    """Find an app named in the command itself ("... no VS Code", "... in Reaper")."""
    m = re.search(r"\b(?:no|na|com|in|with|on)\s+(?:o\s+|a\s+)?(.+?)\s*[.!?]?$", text, re.I)
    if m:
        return find_app(m.group(1))
    return None


def act_open_project(p):
    app = find_app(p["app"]) if p.get("app") else app_in_text(p.get("_command", ""))
    name = p.get("name", "")
    if app:  # drop a trailing "no VS Code" the model may have left in the name
        name = re.sub(r"\s+(no|na|in|with|on)\s+.*$", "", name, flags=re.I)
        p["name"] = name
    ext = ".rpp" if app and "reaper" in norm(app[0]) else None
    path = find_project(p.get("name", ""), ext)
    if not path:
        return f"Não achei o projeto \"{p.get('name', '')}\""
    cmd = ["open", "-a", app[1], path] if app else ["open", path]
    subprocess.run(cmd, check=True)
    where = f" no {app[0]}" if app else ""
    return f"Abrindo {os.path.basename(path)}{where}"


def act_slack_open(p):
    conv = clean_conversation(p.get("conversation"))
    if not conv:
        return "Qual conversa do Slack?"
    subprocess.run(["open", "-a", "Slack"], check=True)
    osa(
        'delay 1.2\n'
        'tell application "System Events"\n'
        '  keystroke "k" using command down\n'
        '  delay 0.6\n'
        f'  keystroke {json.dumps(conv)}\n'
        '  delay 1.0\n'
        '  key code 36\n'
        'end tell'
    )
    return f"Slack: {conv}"


def clean_conversation(conv):
    conv = (conv or "").strip()
    conv = re.sub(r"^(o|a|no|na|do|da|com|pra|para|the|with|to)\s+", "", conv, flags=re.I)
    conv = re.sub(r"^(grupo|canal|conversa|chat|group|channel|conversation|dm)\s+(do|da|de|com|with)?\s*", "", conv, flags=re.I)
    return re.sub(r"^(o|a|com|with)\s+", "", conv.strip(), flags=re.I).strip()


def type_text_applescript(text):
    """Type via the clipboard (works with accents/emoji), keeping it restorable."""
    return (
        f'set the clipboard to {json.dumps(text)}\n'
        'tell application "System Events" to keystroke "v" using command down\n'
    )


# Sending is only allowed when the person SAID so, checked in code (not by the LLM).
SEND_WORDS = re.compile(r"\b(e envia|e manda|envia agora|manda agora|pode enviar|pode mandar|and send|send it|send now)\b", re.I)


def act_whatsapp_open(p):
    conv = clean_conversation(p.get("conversation"))
    if not conv:
        return "Qual conversa do WhatsApp?"
    subprocess.run(["open", "-a", "WhatsApp"], check=True)
    osa(
        'delay 1.5\n'
        'tell application "System Events"\n'
        '  keystroke "f" using command down\n'
        '  delay 0.6\n'
        '  keystroke "a" using command down\n'
        f'  keystroke {json.dumps(conv)}\n'
        '  delay 1.2\n'
        '  key code 125\n'   # down arrow: first result
        '  delay 0.3\n'
        '  key code 36\n'    # return: open chat
        'end tell'
    )
    return f"WhatsApp: {conv}"


def act_send_message(p):
    app = norm(p.get("app") or "")
    msg = (p.get("message") or "").strip()
    if not msg:
        return "Qual mensagem?"
    if "whats" in app or "zap" in app:
        opened = act_whatsapp_open(p)
    elif "slack" in app:
        opened = act_slack_open(p)
    else:
        return "Mandar mensagem por qual app? (Slack ou WhatsApp)"
    if opened.endswith("?"):
        return opened
    send = bool(SEND_WORDS.search(p.get("_command", "")))
    try:
        previous_clipboard = osa("the clipboard as text")
    except RuntimeError:
        previous_clipboard = None
    osa('delay 0.8\n' + type_text_applescript(msg) +
        ('delay 0.3\ntell application "System Events" to key code 36\n' if send else ''))
    if previous_clipboard is not None:
        osa(f"delay 0.3\nset the clipboard to {json.dumps(previous_clipboard)}")
    return f"{opened} — mensagem {'ENVIADA' if send else 'escrita (aperte Enter para enviar)'}"


def act_spotify(p):
    cmd = p.get("command") or ("search" if p.get("query") else "play")
    simple = {"play": "play", "pause": "pause", "next": "next track", "previous": "previous track"}
    if cmd in simple:
        subprocess.run(["open", "-g", "-a", "Spotify"], check=True)
        osa(f'tell application "Spotify" to {simple[cmd]}')
        return {"play": "Tocando", "pause": "Pausado", "next": "Próxima música", "previous": "Música anterior"}[cmd]
    q = (p.get("query") or "").strip()
    if not q:  # model left it empty: take it from the spoken command
        q = re.sub(r"^(toca|tocar|coloca|bota|poe|põe|play|put on)\s+(a|o|as|os|uma|um)?\s*", "",
                   p.get("_command", ""), flags=re.I)
        q = re.sub(r"\s+(no|on|in)\s+spotify.*$", "", q, flags=re.I).strip(" .!?")
    if not q:
        return "O que tocar?"
    subprocess.run(["open", "spotify:search:" + urllib.parse.quote(q)], check=True)
    return f"Spotify: buscando {q} (clique para tocar)"


def act_volume(p):
    level = str(p.get("level") or "").strip().lower()
    if level in ("mute", "unmute"):
        osa(f'set volume output muted {"true" if level == "mute" else "false"}')
        return "Sem som" if level == "mute" else "Som ligado"
    current = int(osa("output volume of (get volume settings)") or 50)
    if level in ("up", "down"):
        new = current + (15 if level == "up" else -15)
    else:
        m = re.search(r"\d+", level)
        if not m:
            return "Qual volume?"
        new = int(m.group())
    new = max(0, min(100, new))
    osa(f"set volume output volume {new} without output muted")
    return f"Volume {new}%"


def act_window(p):
    pos = p.get("position") or "full"
    found = find_app(p["app"]) if p.get("app") else None
    if found:
        subprocess.run(["open", "-a", found[1]], check=True)
        osa("delay 0.6")
    # Usable screen area (below the menu bar) from Finder's desktop bounds.
    b = [int(x) for x in osa('tell application "Finder" to get bounds of window of desktop').split(", ")]
    sw, sh, top = b[2], b[3], 25
    h = sh - top
    frames = {
        "left": (0, top, sw // 2, h), "right": (sw // 2, top, sw - sw // 2, h),
        "top": (0, top, sw, h // 2), "bottom": (0, top + h // 2, sw, h - h // 2),
        "full": (0, top, sw, h), "center": (sw // 6, top + h // 8, sw * 2 // 3, h * 3 // 4),
    }
    x, y, w, hh = frames[pos]
    target = (f'process {json.dumps(found[0])}' if found else
              'first process whose frontmost is true')
    osa(
        'tell application "System Events"\n'
        f'  tell {target}\n'
        f'    set position of front window to {{{x}, {y}}}\n'
        f'    set size of front window to {{{w}, {hh}}}\n'
        '  end tell\n'
        'end tell'
    )
    names = {"left": "à esquerda", "right": "à direita", "full": "em tela cheia",
             "center": "no centro", "top": "em cima", "bottom": "embaixo"}
    return f"{found[0] if found else 'Janela'} {names[pos]}"



def act_chrome_close_tabs(p):
    which = p.get("which") or "all"
    if which == "current":
        osa('tell application "Google Chrome" to close active tab of front window')
        return "Aba atual fechada (Cmd+Shift+T reabre)"
    if which == "others":
        osa('tell application "Google Chrome"\n'
            '  set keepId to id of active tab of front window\n'
            '  close (every tab of front window whose id is not keepId)\n'
            'end tell')
        return "Outras abas fechadas (Cmd+Shift+T reabre)"
    osa('tell application "Google Chrome" to close every window')
    return "Abas do Chrome fechadas (Cmd+Shift+T reabre)"


def act_open_url(p):
    url = (p.get("url") or "").strip()
    if not re.match(r"^https?://[\w.-]+", url):
        return "Endereço inválido"
    subprocess.run(["open", url], check=True)
    return f"Abrindo {urllib.parse.urlparse(url).netloc}"


def act_web_search(p):
    q = (p.get("query") or "").strip()
    if not q:
        return "O que pesquisar?"
    subprocess.run(["open", "https://www.google.com/search?q=" + urllib.parse.quote(q)], check=True)
    return f"Pesquisando: {q}"


def act_unknown(p):
    return "Não entendi o comando"


ACTIONS = {name[4:]: fn for name, fn in globals().items() if name.startswith("act_")}


# ------------------------------------------------------------------ LLM
def choose_action(command):
    apps = ", ".join(sorted(installed_apps()))
    prompt = (
        "You turn a voice command (Portuguese or English) into ONE action for a Mac.\n"
        "Pick the action and fill only its parameters. Use exact app names from the installed list when possible.\n\n"
        f"ACTIONS:\n{ACTION_HELP}\n\nINSTALLED APPS: {apps}\n\n"
        f"COMMAND: {command}\n\nAnswer with JSON only."
    )
    body = {
        "model": MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "format": SCHEMA,
        "stream": False,
        "think": False,
        "keep_alive": -1,
        "options": {"temperature": 0},
    }
    req = urllib.request.Request(OLLAMA_URL, json.dumps(body).encode(), {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(json.load(r)["message"]["content"])


def main():
    command = " ".join(sys.argv[1:]).strip()
    if not command:
        return 1
    dry = os.environ.get("VOCAL_COMMANDS_DRY_RUN") == "1"
    try:
        choice = choose_action(command)
    except Exception as e:  # Ollama down, bad JSON...
        log(f"ERROR choosing action for {command!r}: {e}")
        notify("Comando falhou: a IA local (Ollama) não respondeu")
        return 1
    action = choice.get("action", "unknown")
    fn = ACTIONS.get(action, act_unknown)
    choice["_command"] = command
    log(f"{command!r} -> {json.dumps(choice, ensure_ascii=False)}")
    print(json.dumps(choice, ensure_ascii=False))
    if dry:
        return 0
    try:
        result = fn(choice)
    except Exception as e:
        result = f"Erro em {action}: {e}"
        log(f"ERROR {result}")
    log(f"   => {result}")
    notify(result)
    return 0


if __name__ == "__main__":
    sys.exit(main())
