#!/usr/bin/env python3
"""glibc_env.py — ambiente glibc da camada de compatibilidade Linux do Pulsar.

Programas Linux prontos (o Firefox do site da Mozilla, bash, coreutils...) sao
ligados a glibc e esperam as bibliotecas de uma distribuicao. Esta ferramenta
monta esse ambiente a partir dos pacotes binarios do Debian 13 (trixie), que
suporta as duas arquiteturas do Pulsar (arm64 e riscv64):

  1. baixa o InRelease do Debian e confere a assinatura com gpgv, usando o
     chaveiro do pacote debian-archive-keyring (baixado por HTTPS e conferido
     depois pela propria assinatura);
  2. baixa o indice Packages.xz e confere o SHA-256 contra o InRelease;
  3. resolve as dependencias (Depends/Pre-Depends, alternativas e pacotes
     virtuais) dos pacotes pedidos;
  4. baixa cada .deb, confere o SHA-256 contra o indice e extrai o conteudo
     (so os dados, sem scripts de instalacao) na raiz indicada;
  5. grava um arquivo de trava (pacote, versao, sha256) para builds
     reproduziveis: com --lock, usa exatamente as versoes travadas.

Os arquivos ficam no layout multiarch do Debian (/usr/lib/<tripla>), separados
do ambiente musl do Pulsar (/usr/lib, /lib/ld-musl-*). O carregador fica em
/lib/ld-linux-<arch>.so.1, como os binarios esperam.

Uso: glibc_env.py <aarch64|riscv64> <raiz> <cache> [--lock arq] (pacote | @lista)...
"""
import hashlib
import lzma
import os
import re
import shutil
import subprocess
import sys
import urllib.request

MIRROR = "https://deb.debian.org/debian"
SUITE = "trixie"
ARCHS = {"aarch64": ("arm64", "aarch64-linux-gnu", "ld-linux-aarch64.so.1"),
         "riscv64": ("riscv64", "riscv64-linux-gnu", "ld-linux-riscv64-lp64d.so.1")}
# pacotes que so fazem sentido numa distribuicao instalada (scripts, daemons,
# gerenciador de pacotes): as dependencias deles nao entram no Pulsar
SKIP = {"debconf", "debconf-2.0", "dpkg", "perl-base", "adduser", "passwd", "login",
        "init-system-helpers", "base-files", "base-passwd", "sysvinit-utils", "systemd",
        "systemd-sysv", "libpam-systemd", "dbus-system-bus", "dbus-session-bus", "dbus-daemon",
        "dbus-bin", "default-dbus-session-bus", "default-dbus-system-bus", "install-info",
        "ucf", "usr-is-merged", "usrmerge", "mount", "util-linux", "libpam-runtime",
        "debianutils", "cron", "logsave", "procps", "sensible-utils", "tzdata"}


def fetch(url, cache):
    os.makedirs(cache, exist_ok=True)
    path = os.path.join(cache, url.split("/")[-1])
    if not os.path.exists(path):
        tmp = path + ".part"
        with urllib.request.urlopen(url, timeout=120) as r, open(tmp, "wb") as f:
            shutil.copyfileobj(r, f)
        os.rename(tmp, path)
    return path


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def parse_paragraphs(text):
    pkgs = {}
    for para in text.split("\n\n"):
        d, key = {}, None
        for line in para.split("\n"):
            if line.startswith((" ", "\t")) and key:
                d[key] += "\n" + line.strip()
            elif ":" in line:
                key, _, val = line.partition(":")
                d[key] = val.strip()
        if "Package" in d:
            pkgs[d["Package"]] = d
    return pkgs


def verified_index(arch, cache):
    """InRelease assinado -> Packages.xz conferido -> dicionario de pacotes."""
    # chaveiro do Debian (o proprio InRelease confere que e o certo)
    kr = os.path.join(cache, "debian-archive-keyring.gpg")
    if not os.path.exists(kr):
        pool = MIRROR + "/pool/main/d/debian-archive-keyring/"
        html = urllib.request.urlopen(pool, timeout=60).read().decode()
        debs = sorted(set(re.findall(r'debian-archive-keyring_[0-9.~+a-z]+_all\.deb', html)))
        deb = fetch(pool + debs[-1], cache)
        x = os.path.join(cache, "keyring-x")
        subprocess.run(["dpkg-deb", "-x", deb, x], check=True)
        shutil.copy(os.path.join(x, "usr/share/keyrings/debian-archive-keyring.gpg"), kr)
    inrel = fetch(f"{MIRROR}/dists/{SUITE}/InRelease", cache)
    r = subprocess.run(["gpgv", "--keyring", kr, inrel], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit("assinatura do InRelease do Debian NAO confere:\n" + r.stderr)
    text = open(inrel).read()
    want = f"main/binary-{arch}/Packages.xz"
    m = re.search(r"^SHA256:\n((?: .*\n)+)", text, re.M)
    digest = None
    for line in m.group(1).splitlines():
        h, _, name = line.split()
        if name == want:
            digest = h
    pk = fetch(f"{MIRROR}/dists/{SUITE}/{want}", os.path.join(cache, arch))
    if sha256(pk) != digest:
        sys.exit(f"{want}: SHA-256 nao confere com o InRelease")
    return parse_paragraphs(lzma.open(pk).read().decode())


def resolve(pkgs, roots):
    provides = {}
    for name, d in pkgs.items():
        for p in d.get("Provides", "").split(","):
            p = p.strip().split(" ")[0]
            if p:
                provides.setdefault(p, name)
    out, todo = {}, list(roots)
    while todo:
        name = todo.pop()
        if name in out or name in SKIP:
            continue
        real = name if name in pkgs else provides.get(name)
        if real is None:
            sys.exit(f"pacote inexistente: {name}")
        if real in out or real in SKIP:
            continue
        out[real] = pkgs[real]
        deps = pkgs[real].get("Pre-Depends", "") + "," + pkgs[real].get("Depends", "")
        for dep in deps.split(","):
            alts = [a.strip().split(" ")[0].split(":")[0] for a in dep.split("|") if a.strip()]
            if not alts:
                continue
            pick = next((a for a in alts if a in pkgs or a in provides), None)
            if pick and not any(a in out for a in alts):
                todo.append(pick)
    return out


# entradas do loaders.cache para modulos que o host nao tenha instalado
# (texto que o gdk-pixbuf-query-loaders gera a partir do proprio modulo)
EXTRA_LOADERS = {
    # librsvg (librsvg2-common): o tema Adwaita do GTK desenha com SVG
    "libpixbufloader_svg.so": '"svg" 6 "gdk-pixbuf" "Scalable Vector Graphics" "LGPL"\n'
        '"image/svg+xml" "image/svg" "image/svg-xml" "image/vnd.adobe.svg+xml" "text/xml-svg" '
        '"image/svg+xml-compressed" ""\n'
        '"svg" "svgz" "svg.gz" ""\n'
        '" <svg" "*    " 100\n'
        '" <!DOCTYPE svg" "*             " 100',
}


def pixbuf_cache(root, triplet):
    """loaders.cache do gdk-pixbuf (no Debian quem gera e um script de
    instalacao, que aqui nao roda): texto com o caminho de cada modulo e os
    formatos dele. Vem do cache do proprio host (mesma serie do gdk-pixbuf),
    com os caminhos trocados para a tripla do alvo e so os modulos que
    existem nela."""
    d = os.path.join(root, "usr/lib", triplet, "gdk-pixbuf-2.0/2.10.0")
    if not os.path.isdir(os.path.join(d, "loaders")):
        return
    import glob
    host = (glob.glob("/usr/lib/*-linux-gnu/gdk-pixbuf-2.0/2.10.0/loaders.cache") or [None])[0]
    if not host:
        print("[glibc] aviso: sem loaders.cache no host; o GTK nao vai abrir imagens alem de PNG/JPEG")
        return
    have = set(os.listdir(os.path.join(d, "loaders")))
    blocks = open(host).read().split("\n\n")
    out = ["# gerado por tools/glibc_env.py a partir de " + host]
    done = set()
    for b in blocks:
        m = re.search(r'^"([^"]+/loaders/([^"/]+))"', b, re.M)
        if not m or m.group(2) not in have:
            continue
        done.add(m.group(2))
        out.append(b.replace(m.group(1), f"/usr/lib/{triplet}/gdk-pixbuf-2.0/2.10.0/loaders/{m.group(2)}").strip())
    # modulos que o host pode nao ter: os dados que o proprio modulo declara
    for mod, info in EXTRA_LOADERS.items():
        if mod in have and mod not in done:
            out.append(f'"/usr/lib/{triplet}/gdk-pixbuf-2.0/2.10.0/loaders/{mod}"\n' + info)
    # cada entrada termina numa linha em branco: e nela que o gdk-pixbuf
    # registra o modulo (sem ela a ultima entrada e descartada)
    open(os.path.join(d, "loaders.cache"), "w").write("\n\n".join(out) + "\n\n")


def main():
    args = sys.argv[1:]
    if len(args) < 4 or args[0] not in ARCHS:
        sys.exit(__doc__)
    arch, root, cache = args[0], args[1], args[2]
    lock = None
    rest = args[3:]
    if rest[:1] == ["--lock"]:
        lock, rest = rest[1], rest[2:]
    # "@arquivo": lista de pacotes (um por linha, '#' comenta)
    roots = []
    for r in rest:
        if r.startswith("@"):
            roots += [l.split("#")[0].strip() for l in open(r[1:]) if l.split("#")[0].strip()]
        else:
            roots.append(r)
    rest = roots
    darch, triplet, ldso = ARCHS[arch]
    pkgs = verified_index(darch, cache)
    chosen = resolve(pkgs, rest)
    pinned = {}
    if lock and os.path.exists(lock):
        for line in open(lock):
            if line.strip() and not line.startswith("#"):
                n, v, h = line.split()
                pinned[n] = (v, h)
    os.makedirs(root, exist_ok=True)
    entries = []
    for name in sorted(chosen):
        d = chosen[name]
        ver, digest = d["Version"], d["SHA256"]
        if name in pinned and pinned[name] != (ver, digest):
            sys.exit(f"{name}: o indice tem {ver}, a trava pede {pinned[name][0]} (rode sem --lock para atualizar)")
        deb = fetch(f"{MIRROR}/{d['Filename']}", os.path.join(cache, darch))
        if sha256(deb) != digest:
            sys.exit(f"{name}: SHA-256 do .deb nao confere")
        subprocess.run(["dpkg-deb", "-x", deb, root], check=True)
        entries.append(f"{name} {ver} {digest}")
    # carregador no caminho que os binarios pedem (PT_INTERP)
    os.makedirs(os.path.join(root, "lib"), exist_ok=True)
    link = os.path.join(root, "lib", ldso)
    if os.path.lexists(link):
        os.remove(link)
    os.symlink(f"/usr/lib/{triplet}/{ldso}", link)
    # o que os scripts de instalacao do Debian gerariam (aqui eles nao rodam)
    pixbuf_cache(root, triplet)
    mime = os.path.join(root, "usr/share/mime")
    if os.path.isdir(os.path.join(mime, "packages")):
        # banco do shared-mime-info (independe da arquitetura): o gdk-pixbuf
        # do Debian reconhece os formatos de imagem por ele
        subprocess.run(["update-mime-database", mime], check=True)
    # indice dos temas de icones (gatilho do gtk-update-icon-cache no Debian):
    # sem ele o GTK varre os diretorios do tema a cada programa aberto
    icons = os.path.join(root, "usr/share/icons")
    for theme in sorted(os.listdir(icons)) if os.path.isdir(icons) else []:
        t = os.path.join(icons, theme)
        if os.path.isfile(os.path.join(t, "index.theme")) and shutil.which("gtk-update-icon-cache"):
            subprocess.run(["gtk-update-icon-cache", "-q", "-f", "-t", t], check=True)
    if lock:
        with open(lock, "w") as f:
            f.write(f"# ambiente glibc do Pulsar ({arch}): Debian {SUITE}, gerado por tools/glibc_env.py\n")
            f.write("\n".join(entries) + "\n")
    print(f"[glibc] {arch}: {len(entries)} pacotes do Debian {SUITE} em {root}")


if __name__ == "__main__":
    main()
