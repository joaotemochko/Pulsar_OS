#!/bin/sh
# firefox_install.sh <arch> <raiz> <cache> <glibcroot>
#
# Firefox pre-instalado no Pulsar (navegador padrao por ora), rodando na
# camada Linux sobre o ambiente glibc (make glibc):
#
#   aarch64  o pacote OFICIAL da Mozilla (o mesmo do site), conferido por
#            SHA-256, extraido em /opt/firefox -- como num Linux qualquer;
#   riscv64  a Mozilla nao publica Firefox para RISC-V: vem o firefox-esr do
#            Debian, que entra pelo ambiente glibc (user/glibc/pacotes-riscv64.txt,
#            assinatura do Debian conferida); aqui so o atalho e o icone.
#
# Em <raiz> (copiada para o disco): /usr/bin/firefox e o icone do proprio
# Firefox convertido para o shell (/usr/share/pulsar/icons/firefox.icn).
set -e
ARCH=$1; ROOT=$2; CACHE=$3; GLIBC_ROOT=$4
TOOLS=$(cd "$(dirname "$0")" && pwd)
VER=153.5.0esr
LANG_FF=pt-BR
SHA_aarch64=520eba74f41692a856367130dfc1bee9bae64060609e864c6869b2513a6067e5

rm -rf "$ROOT" && mkdir -p "$ROOT/usr/bin" "$ROOT/usr/share/pulsar/icons"
case $ARCH in
aarch64)
    T=$CACHE/firefox-$VER-$ARCH-$LANG_FF.tar.xz
    mkdir -p "$CACHE"
    if [ ! -f "$T" ]; then
        curl -sfo "$T.part" \
            "https://archive.mozilla.org/pub/firefox/releases/$VER/linux-aarch64/$LANG_FF/firefox-$VER.tar.xz"
        mv "$T.part" "$T"
    fi
    echo "$SHA_aarch64  $T" | sha256sum -c --quiet || { echo "firefox: SHA-256 nao confere"; exit 1; }
    mkdir -p "$ROOT/opt" && tar xJf "$T" -C "$ROOT/opt"
    rm -f "$ROOT/opt/firefox/glxtest" "$ROOT/opt/firefox/vaapitest" "$ROOT/opt/firefox/v4l2test" \
          "$ROOT/opt/firefox/vulkantest" "$ROOT/opt/firefox/crashreporter" "$ROOT/opt/firefox/pingsender" \
          "$ROOT/opt/firefox/updater"
    PREF_DIR="$ROOT/opt/firefox/defaults/pref"
    ln -s /opt/firefox/firefox "$ROOT/usr/bin/firefox"
    ICON=$ROOT/opt/firefox/browser/chrome/icons/default/default128.png
    ;;
riscv64)
    [ -e "$GLIBC_ROOT/usr/lib/firefox-esr/firefox-esr" ] || { echo "firefox: falta o firefox-esr no ambiente glibc"; exit 1; }
    ln -s /usr/lib/firefox-esr/firefox-esr "$ROOT/usr/bin/firefox"
    # o Debian escolhe a lingua pelo LANG (C.UTF-8 no Pulsar = ingles): no
    # arquivo de preferencias dele, pede o pt-BR do pacote de traducao (o
    # Firefox da Mozilla ja vem em pt-BR)
    mkdir -p "$ROOT/etc/firefox-esr"
    sed 's|^pref("intl.locale.requested", "");|pref("intl.locale.requested", "pt-BR"); // Pulsar OS|' \
        "$GLIBC_ROOT/etc/firefox-esr/firefox-esr.js" > "$ROOT/etc/firefox-esr/firefox-esr.js"
    grep -q '"pt-BR"' "$ROOT/etc/firefox-esr/firefox-esr.js" || { echo "firefox: preferencia de lingua nao achada"; exit 1; }
    PREF_DIR="$ROOT/usr/lib/firefox-esr/defaults/pref"
    ICON=$GLIBC_ROOT/usr/share/icons/hicolor/128x128/apps/firefox-esr.png
    ;;
*) echo "arquitetura sem Firefox: $ARCH"; exit 1 ;;
esac

mkdir -p "$PREF_DIR"
cat > "$PREF_DIR/pulsar.js" << 'EOF'
// Preferencias de desempenho do Firefox para o Pulsar OS
pref("fission.autostart", false);
pref("dom.ipc.processCount", 1);
pref("dom.ipc.processCount.webIsolated", 1);
pref("dom.ipc.processCount.extension", 1);
pref("dom.ipc.processCount.privilegedabout", 1);
pref("dom.ipc.processCount.privilegedmozilla", 1);
pref("dom.ipc.processCount.file", 1);
pref("dom.ipc.processPrelaunch.enabled", false);
pref("gfx.webrender.all", true);
pref("gfx.webrender.software", true);
pref("layers.acceleration.disabled", true);
pref("media.hardware-video-decoding.enabled", false);
pref("widget.wayland.vsync.enabled", false);
pref("toolkit.cosmeticAnimations.enabled", false);
pref("ui.prefersReducedMotion", 1);
pref("browser.tabs.animate", false);
pref("browser.fullscreen.animate", false);
pref("browser.startup.homepage", "about:blank");
pref("browser.startup.page", 0);
pref("browser.shell.checkDefaultBrowser", false);
pref("browser.newtabpage.enabled", false);
pref("browser.newtabpreload", false);
pref("browser.aboutwelcome.enabled", false);
pref("trailhead.firstrun.didSeeAboutWelcome", true);
pref("datareporting.policy.dataSubmissionEnabled", false);
pref("datareporting.healthreport.uploadEnabled", false);
pref("toolkit.telemetry.enabled", false);
pref("toolkit.telemetry.unified", false);
pref("toolkit.telemetry.archive.enabled", false);
pref("app.shield.optoutstudies.enabled", false);
pref("normandy.enabled", false);
pref("browser.safebrowsing.malware.enabled", false);
pref("browser.safebrowsing.phishing.enabled", false);
pref("browser.safebrowsing.downloads.enabled", false);
pref("network.dns.disableIPv6", true);
pref("network.http.speculative-parallel-limit", 0);
pref("network.prefetch-next", false);
pref("accessibility.force_disabled", 1);
EOF

python3 "$TOOLS/png2icn.py" "$ICON" "$ROOT/usr/share/pulsar/icons/firefox.icn"
echo "[firefox] $ARCH: instalado em $ROOT"
