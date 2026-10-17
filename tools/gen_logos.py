#!/usr/bin/env python3
"""Builds the bundled logo library: assets/logos/*.png and src/logos_table.rs.

Source: Simple Icons (CC0, https://simpleicons.org). Each glyph is drawn white (or dark on light
brand colors) over a rounded tile in the brand color, 80px, palette PNG to stay tiny.
Brand names and logos belong to their owners; they are only used to identify the software.

    python3 -m venv v && v/bin/pip install cairosvg pillow && v/bin/python tools/gen_logos.py
"""
import io, json, re, sys, urllib.request, urllib.error
from pathlib import Path
import cairosvg
from PIL import Image, ImageDraw

VERSION = "13.21.0"
CDN = f"https://cdn.jsdelivr.net/npm/simple-icons@{VERSION}"
ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "logos"
SIZE, GLYPH = 80, 48

# simple-icons slug -> extra names (package tokens / display names) that should show it.
# The slug itself is always an alias. Names are matched after lowercasing and dropping non-alphanumerics.
LIB = {
    "docker": ["docker-buildx", "docker-compose", "docker-desktop", "docker-credential-helper", "colima", "lima"],
    "flutter": ["flutter-sdk"], "dart": [], "amazonaws": ["awscli", "aws", "aws-cli"],
    "gnubash": ["bash"], "cocoapods": [], "composer": [], "git": ["git-lfs", "git-flow"],
    "github": ["gh", "github-desktop"], "gitlab": ["glab"], "gnu": ["gnu-sed", "gnu-tar", "coreutils", "wget", "grep", "gawk", "findutils"],
    "go": ["golang", "golangci-lint", "gopls", "goreleaser"], "openjdk": ["java", "jdk"], "pnpm": [],
    "rust": ["rustup", "rustc", "cargo"], "7zip": ["sevenzip", "p7zip"], "yarn": [],
    "nodedotjs": ["node", "nodejs"], "npm": [], "python": ["python3", "pip", "pipx"], "ruby": ["rbenv"],
    "php": [], "kotlin": [], "swift": ["swiftlint", "swiftformat"], "typescript": [], "deno": [], "bun": [],
    "kubernetes": ["kubectl", "kubernetes-cli", "kubectx", "k9s", "minikube"], "helm": [], "terraform": ["tfenv"],
    "ansible": [], "vagrant": [], "postgresql": ["postgres", "libpq"], "mysql": [], "mariadb": [], "redis": [],
    "mongodb": ["mongosh"], "sqlite": [], "nginx": [], "apache": ["httpd"], "curl": [], "vim": [],
    "neovim": ["nvim"], "tmux": [], "zsh": [], "fishshell": ["fish"], "ffmpeg": [], "imagemagick": [],
    "cmake": [], "gradle": [], "apachemaven": ["maven"], "androidstudio": [], "android": ["android-platform-tools", "scrcpy"],
    "firefox": [], "googlechrome": ["chrome", "google-chrome"], "brave": [], "visualstudiocode": ["vscode", "visual-studio-code"],
    "vlcmediaplayer": ["vlc"], "spotify": [], "discord": [], "telegram": [], "signal": [], "zoom": [],
    "notion": [], "obsidian": [], "figma": [], "postman": [], "iterm2": [], "raycast": [], "1password": [],
    "bitwarden": [], "dropbox": [], "slack": [], "whatsapp": [], "xcode": [], "appstore": ["mas"],
    "tailscale": [], "ngrok": [], "cloudflare": ["cloudflared", "wrangler"], "homebrew": ["brew"],
    "blender": [], "godot": ["godot-engine"], "unity": [], "intellijidea": ["intellij-idea", "intellij-idea-ce"],
    "pycharm": [], "webstorm": [], "sublimetext": ["sublime-text"], "vscodium": [], "openai": ["chatgpt", "codex"],
    "claude": ["claude-code"], "anthropic": [], "ollama": [], "huggingface": [], "sentry": [], "grafana": [],
    "prometheus": [], "elasticsearch": [], "rabbitmq": [], "apachekafka": ["kafka"], "youtube": ["yt-dlp", "youtube-dl"],
    "linear": [], "arc": [], "sketch": [], "affinity": [], "cursor": [], "warp": [], "firebase": ["firebase-cli"],
    "supabase": [], "vercel": [], "netlify": ["netlify-cli"], "heroku": [], "digitalocean": ["doctl"],
    "googlecloud": ["gcloud", "google-cloud-sdk"], "microsoftazure": ["azure-cli", "az"], "jq": [], "wireshark": [],
    "nmap": [], "virtualbox": [], "vmware": [], "qemu": [], "balenaetcher": ["etcher"], "obsproject": ["obs"],
    "audacity": [], "gimp": [], "inkscape": [], "krita": [], "libreoffice": [], "thunderbird": [], "transmission": [],
    "qbittorrent": [], "steam": [], "epicgames": [], "nvidia": [], "tesseract": [], "latex": ["mactex", "texlive"],
    "pandoc": [], "rubygems": [], "bundler": [], "rust": ["rustup", "rustc", "cargo"],
}

def norm(s): return re.sub(r"[^a-z0-9]", "", s.lower())

def fetch(url):
    try:
        return urllib.request.urlopen(url, timeout=30).read()
    except urllib.error.HTTPError:
        return None

def luminance(hex_):
    r, g, b = (int(hex_[i:i + 2], 16) / 255 for i in (0, 2, 4))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b

def tile(svg, hex_):
    bg = hex_ if luminance(hex_) > 0.08 else "30343B"  # near-black brands vanish on the dark UI
    fg = "#111111" if luminance(hex_) > 0.7 else "#FFFFFF"
    svg = svg.decode().replace("<path ", f'<path fill="{fg}" ', 1)
    glyph = Image.open(io.BytesIO(cairosvg.svg2png(bytestring=svg.encode(), output_width=GLYPH, output_height=GLYPH))).convert("RGBA")
    img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ImageDraw.Draw(img).rounded_rectangle((0, 0, SIZE - 1, SIZE - 1), radius=int(SIZE * 0.22), fill="#" + bg)
    img.alpha_composite(glyph, ((SIZE - GLYPH) // 2, (SIZE - GLYPH) // 2))
    buf = io.BytesIO()
    img.quantize(colors=48, method=Image.Quantize.FASTOCTREE).save(buf, "PNG", optimize=True)
    return buf.getvalue()

def main():
    data = json.loads(fetch(f"{CDN}/_data/simple-icons.json"))
    colors = {e["title"]: e["hex"] for e in (data["icons"] if isinstance(data, dict) else data)}
    OUT.mkdir(parents=True, exist_ok=True)
    rows, missing = [], []
    for slug, extra in sorted(LIB.items()):
        svg = fetch(f"{CDN}/icons/{slug}.svg")
        if not svg:
            missing.append(slug)
            continue
        title = re.search(rb"<title>(.*?)</title>", svg).group(1).decode().replace("&amp;", "&")
        hex_ = colors.get(title)
        if not hex_:
            missing.append(slug + "(color)")
            continue
        png = tile(svg, hex_)
        (OUT / f"{slug}.png").write_bytes(png)
        aliases = sorted({norm(slug), *map(norm, extra)})
        rows.append((slug, aliases))
    for f in OUT.glob("*.png"):
        if f.stem not in {r[0] for r in rows}:
            f.unlink()
    lines = ["// Generated by tools/gen_logos.py. Do not edit.", "pub static LOGOS: &[(&[&str], &[u8])] = &["]
    for slug, aliases in rows:
        names = ", ".join(f'"{a}"' for a in aliases)
        lines.append(f'    (&[{names}], include_bytes!("../assets/logos/{slug}.png")),')
    lines.append("];\n")
    (ROOT / "src" / "logos_table.rs").write_text("\n".join(lines))
    total = sum(f.stat().st_size for f in OUT.glob("*.png"))
    print(f"{len(rows)} logos, {total/1024:.0f} KB; missing: {', '.join(missing) or 'none'}")

main()
