#!/usr/bin/env python3
"""genkeymaps.py — fonte unica dos layouts de teclado do Pulsar OS.

Gera, a partir das tabelas abaixo:
  user/plib/src/keymaps.rs                   tabelas do teclado nativo (WM)
  rootfs/usr/share/pulsar/xkb/<layout>.xkb   mapas XKB completos, enviados
                                             aos programas Wayland (wlclient)
  rootfs/usr/share/pulsar/Compose            teclas mortas no formato XCompose

Layouts: abnt, abnt2 (Brasil), us, us-intl (EUA internacional, com teclas
mortas). Os mapas XKB sao validados com o xkbcomp do host, se existir.

Uso: python3 tools/genkeymaps.py
"""
import os, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# ---------------------------------------------------------------- simbolos

# caractere -> nome do keysym X11
NAMES = {
    ' ': 'space', '!': 'exclam', '"': 'quotedbl', '#': 'numbersign', '$': 'dollar', '%': 'percent',
    '&': 'ampersand', "'": 'apostrophe', '(': 'parenleft', ')': 'parenright', '*': 'asterisk',
    '+': 'plus', ',': 'comma', '-': 'minus', '.': 'period', '/': 'slash', ':': 'colon',
    ';': 'semicolon', '<': 'less', '=': 'equal', '>': 'greater', '?': 'question', '@': 'at',
    '[': 'bracketleft', '\\': 'backslash', ']': 'bracketright', '^': 'asciicircum', '_': 'underscore',
    '`': 'grave', '{': 'braceleft', '|': 'bar', '}': 'braceright', '~': 'asciitilde',
    '¡': 'exclamdown', '¢': 'cent', '£': 'sterling', '¤': 'currency', '¥': 'yen', '§': 'section',
    '¨': 'diaeresis', 'ª': 'ordfeminine', '¬': 'notsign', '°': 'degree', '²': 'twosuperior',
    '³': 'threesuperior', '´': 'acute', '¹': 'onesuperior', 'º': 'masculine', '¼': 'onequarter',
    '½': 'onehalf', '¾': 'threequarters', '¿': 'questiondown', '×': 'multiply', '÷': 'division',
    'ß': 'ssharp', 'ç': 'ccedilla', 'Ç': 'Ccedilla', 'á': 'aacute', 'Á': 'Aacute', 'é': 'eacute',
    'É': 'Eacute', 'í': 'iacute', 'Í': 'Iacute', 'ó': 'oacute', 'Ó': 'Oacute', 'ú': 'uacute',
    'Ú': 'Uacute', 'ñ': 'ntilde', 'Ñ': 'Ntilde', 'ü': 'udiaeresis', 'Ü': 'Udiaeresis',
    'ö': 'odiaeresis', 'Ö': 'Odiaeresis', 'ä': 'adiaeresis', 'Ä': 'Adiaeresis', '€': 'EuroSign',
    '¶': 'paragraph', '©': 'copyright', '®': 'registered', 'µ': 'mu', 'æ': 'ae', 'Æ': 'AE',
    'å': 'aring', 'Å': 'Aring', 'ø': 'oslash', 'Ø': 'Oslash', 'ð': 'eth', 'Ð': 'ETH',
    'þ': 'thorn', 'Þ': 'THORN', '«': 'guillemotleft', '»': 'guillemotright',
}
# teclas mortas: token -> (keysym, caractere "isolado" usado no teclado nativo)
DEAD = {
    'DA': ('dead_acute', '´'), 'DG': ('dead_grave', '`'), 'DC': ('dead_circumflex', '^'),
    'DT': ('dead_tilde', '~'), 'DD': ('dead_diaeresis', '¨'),
}

def keysym(tok):
    if tok in DEAD: return DEAD[tok][0]
    if len(tok) == 1 and (tok.isalnum() and tok.isascii()): return tok
    if tok in NAMES: return NAMES[tok]
    if len(tok) > 1: return tok          # ja e um nome (ex.: KP_1)
    raise SystemExit(f'sem keysym para {tok!r}')

# ---------------------------------------------------------------- layouts
# Cada tecla imprimivel: codigo evdev -> niveis [base, shift, altgr, altgr+shift]
# (strings de um caractere, ou tokens de tecla morta DA/DG/DC/DT/DD, ou None).

LETTERS = {16: 'q', 17: 'w', 18: 'e', 19: 'r', 20: 't', 21: 'y', 22: 'u', 23: 'i', 24: 'o', 25: 'p',
           30: 'a', 31: 's', 32: 'd', 33: 'f', 34: 'g', 35: 'h', 36: 'j', 37: 'k', 38: 'l',
           44: 'z', 45: 'x', 46: 'c', 47: 'v', 48: 'b', 49: 'n', 50: 'm'}

def base_us():
    k = {c: [l, l.upper(), None, None] for c, l in LETTERS.items()}
    row = '1234567890-='; shifted = '!@#$%^&*()_+'
    for i, (a, b) in enumerate(zip(row, shifted)): k[2 + i] = [a, b, None, None]
    k.update({26: ['[', '{', None, None], 27: [']', '}', None, None], 39: [';', ':', None, None],
              40: ["'", '"', None, None], 41: ['`', '~', None, None], 43: ['\\', '|', None, None],
              51: [',', '<', None, None], 52: ['.', '>', None, None], 53: ['/', '?', None, None],
              86: ['\\', '|', None, None]})
    return k

def layout_us():
    return base_us()

def layout_us_intl():
    k = base_us()
    k[40] = ['DA', 'DD', "'", '"']                       # ' e " mortos
    k[41] = ['DG', 'DT', '`', '~']                       # ` e ~ mortos
    k[7] = ['6', 'DC', '¼', None]                        # ^ morto
    k[2] = ['1', '!', '¡', '¹']; k[3] = ['2', '@', '²', None]; k[4] = ['3', '#', '³', None]
    k[5] = ['4', '$', '¤', '£']; k[6] = ['5', '%', '€', None]; k[13] = ['=', '+', '×', '÷']
    k[53] = ['/', '?', '¿', None]; k[51] = [',', '<', 'ç', 'Ç']
    for code, (lo, hi) in {18: ('é', 'É'), 22: ('ú', 'Ú'), 23: ('í', 'Í'), 24: ('ó', 'Ó'), 30: ('á', 'Á'),
                           49: ('ñ', 'Ñ'), 21: ('ü', 'Ü'), 25: ('ö', 'Ö'), 16: ('ä', 'Ä'), 31: ('ß', '§'),
                           46: ('©', None), 19: ('®', None)}.items():
        k[code][2:] = [lo, hi]
    return k

def layout_abnt2():
    k = {c: [l, l.upper(), None, None] for c, l in LETTERS.items()}
    k.update({
        2: ['1', '!', '¹', None], 3: ['2', '@', '²', None], 4: ['3', '#', '³', None], 5: ['4', '$', '£', None],
        6: ['5', '%', '¢', None], 7: ['6', 'DD', '¬', None], 8: ['7', '&', None, None], 9: ['8', '*', None, None],
        10: ['9', '(', None, None], 11: ['0', ')', None, None], 12: ['-', '_', None, None], 13: ['=', '+', '§', None],
        26: ['DA', 'DG', None, None], 27: ['[', '{', 'ª', None], 39: ['ç', 'Ç', None, None],
        40: ['DT', 'DC', None, None], 41: ["'", '"', None, None], 43: [']', '}', 'º', None],
        51: [',', '<', None, None], 52: ['.', '>', None, None], 53: [';', ':', None, None],
        86: ['\\', '|', None, None], 89: ['/', '?', '°', None],
    })
    k[16][2] = '/'; k[17][2] = '?'; k[18][2] = '°'           # AltGr+Q W E
    return k

def layout_abnt():
    k = layout_abnt2()
    del k[89]                                                  # sem a tecla extra do ABNT2
    return k

LAYOUTS = {
    'abnt': ('Portuguese (Brazil, ABNT)', layout_abnt(), True),
    'abnt2': ('Portuguese (Brazil, ABNT2)', layout_abnt2(), True),
    'us': ('English (US)', layout_us(), False),
    'us-intl': ('English (US, international with dead keys)', layout_us_intl(), True),
}
ALPHA = set(LETTERS) | {39}          # caps lock vale para letras (e o ç do ABNT)

# ---------------------------------------------------------------- compose

BASES = 'aeiouAEIOUncNCyY '
def compose_table():
    acc = {
        'DA': dict(a='á', e='é', i='í', o='ó', u='ú', A='Á', E='É', I='Í', O='Ó', U='Ú', y='ý', Y='Ý',
                   c='ç', C='Ç'),                          # como no Brasil: ´ + c = ç
        'DG': dict(a='à', e='è', i='ì', o='ò', u='ù', A='À', E='È', I='Ì', O='Ò', U='Ù'),
        'DC': dict(a='â', e='ê', i='î', o='ô', u='û', A='Â', E='Ê', I='Î', O='Ô', U='Û'),
        'DT': dict(a='ã', o='õ', n='ñ', A='Ã', O='Õ', N='Ñ'),
        'DD': dict(a='ä', e='ë', i='ï', o='ö', u='ü', A='Ä', E='Ë', I='Ï', O='Ö', U='Ü', y='ÿ'),
    }
    rows = []
    for d, m in acc.items():
        for b, r in m.items(): rows.append((d, b, r))
        rows.append((d, ' ', DEAD[d][1]))                  # morta + espaco = o acento
        rows.append((d, d, DEAD[d][1]))                    # morta duas vezes = o acento
    return rows

# ---------------------------------------------------------------- XKB

KEYNAMES = {1: 'ESC', 14: 'BKSP', 15: 'TAB', 28: 'RTRN', 29: 'LCTL', 41: 'TLDE', 42: 'LFSH', 43: 'BKSL',
            54: 'RTSH', 55: 'KPMU', 56: 'LALT', 57: 'SPCE', 58: 'CAPS', 69: 'NMLK', 70: 'SCLK',
            71: 'KP7', 72: 'KP8', 73: 'KP9', 74: 'KPSU', 75: 'KP4', 76: 'KP5', 77: 'KP6', 78: 'KPAD',
            79: 'KP1', 80: 'KP2', 81: 'KP3', 82: 'KP0', 83: 'KPDL', 86: 'LSGT', 87: 'FK11', 88: 'FK12',
            89: 'AB11', 96: 'KPEN', 97: 'RCTL', 98: 'KPDV', 99: 'PRSC', 100: 'RALT', 102: 'HOME', 103: 'UP',
            104: 'PGUP', 105: 'LEFT', 106: 'RGHT', 107: 'END', 108: 'DOWN', 109: 'PGDN', 110: 'INS',
            111: 'DELE', 119: 'PAUS', 125: 'LWIN', 126: 'RWIN', 127: 'COMP'}
for i in range(12): KEYNAMES[2 + i] = f'AE{i+1:02d}'
for i in range(12): KEYNAMES[16 + i] = f'AD{i+1:02d}'
for i in range(11): KEYNAMES[30 + i] = f'AC{i+1:02d}'
for i in range(10): KEYNAMES[44 + i] = f'AB{i+1:02d}'
for i in range(10): KEYNAMES[59 + i] = f'FK{i+1:02d}'

FIXED = {1: 'Escape', 14: 'BackSpace', 15: ['Tab', 'ISO_Left_Tab'], 28: 'Return', 29: 'Control_L', 42: 'Shift_L',
         54: 'Shift_R', 56: 'Alt_L', 57: 'space', 58: 'Caps_Lock', 69: 'Num_Lock', 70: 'Scroll_Lock',
         55: 'KP_Multiply', 74: 'KP_Subtract', 78: 'KP_Add', 96: 'KP_Enter', 97: 'Control_R', 98: 'KP_Divide',
         99: 'Print', 102: 'Home', 103: 'Up', 104: 'Prior', 105: 'Left', 106: 'Right', 107: 'End',
         108: 'Down', 109: 'Next', 110: 'Insert', 111: 'Delete', 119: 'Pause', 125: 'Super_L', 126: 'Super_R',
         127: 'Menu', 87: 'F11', 88: 'F12'}
for i in range(10): FIXED[59 + i] = f'F{i+1}'
KEYPAD = {71: ('KP_Home', 'KP_7'), 72: ('KP_Up', 'KP_8'), 73: ('KP_Prior', 'KP_9'), 75: ('KP_Left', 'KP_4'),
          76: ('KP_Begin', 'KP_5'), 77: ('KP_Right', 'KP_6'), 79: ('KP_End', 'KP_1'), 80: ('KP_Down', 'KP_2'),
          81: ('KP_Next', 'KP_3'), 82: ('KP_Insert', 'KP_0')}

XKB_TYPES = '''    xkb_types "pulsar" {
        virtual_modifiers NumLock,Alt,LevelThree,Super;
        type "ONE_LEVEL" { modifiers= none; level_name[Level1]= "Any"; };
        type "TWO_LEVEL" { modifiers= Shift; map[Shift]= Level2;
            level_name[Level1]= "Base"; level_name[Level2]= "Shift"; };
        type "ALPHABETIC" { modifiers= Shift+Lock; map[Shift]= Level2; map[Lock]= Level2;
            level_name[Level1]= "Base"; level_name[Level2]= "Caps"; };
        type "KEYPAD" { modifiers= Shift+NumLock; map[None]= Level1; map[Shift]= Level2;
            map[NumLock]= Level2; map[Shift+NumLock]= Level1;
            level_name[Level1]= "Base"; level_name[Level2]= "Number"; };
        type "FOUR_LEVEL" { modifiers= Shift+LevelThree; map[None]= Level1; map[Shift]= Level2;
            map[LevelThree]= Level3; map[Shift+LevelThree]= Level4;
            level_name[Level1]= "Base"; level_name[Level2]= "Shift";
            level_name[Level3]= "Alt Base"; level_name[Level4]= "Shift Alt"; };
        type "FOUR_LEVEL_SEMIALPHABETIC" { modifiers= Shift+Lock+LevelThree; map[None]= Level1;
            map[Shift]= Level2; map[Lock]= Level2; map[LevelThree]= Level3; map[Shift+LevelThree]= Level4;
            map[Lock+LevelThree]= Level3; map[Lock+Shift+LevelThree]= Level4;
            preserve[Lock+LevelThree]= Lock; preserve[Lock+Shift+LevelThree]= Lock;
            level_name[Level1]= "Base"; level_name[Level2]= "Shift";
            level_name[Level3]= "Alt Base"; level_name[Level4]= "Shift Alt"; };
    };
'''

XKB_COMPAT = '''    xkb_compatibility "pulsar" {
        virtual_modifiers NumLock,Alt,LevelThree,Super;
        interpret.useModMapMods= AnyLevel;
        interpret.repeat= False;
        interpret ISO_Level3_Shift+AnyOf(all) { virtualModifier= LevelThree; useModMapMods= level1;
            action= SetMods(modifiers=LevelThree,clearLocks); };
        interpret Caps_Lock+AnyOfOrNone(all) { action= LockMods(modifiers=Lock); };
        interpret Num_Lock+AnyOf(all) { virtualModifier= NumLock; action= LockMods(modifiers=NumLock); };
        interpret Alt_L+AnyOf(all) { virtualModifier= Alt; action= SetMods(modifiers=modMapMods,clearLocks); };
        interpret Alt_R+AnyOf(all) { virtualModifier= Alt; action= SetMods(modifiers=modMapMods,clearLocks); };
        interpret Super_L+AnyOf(all) { virtualModifier= Super; action= SetMods(modifiers=modMapMods,clearLocks); };
        interpret Super_R+AnyOf(all) { virtualModifier= Super; action= SetMods(modifiers=modMapMods,clearLocks); };
        interpret Any+AnyOf(all) { action= SetMods(modifiers=modMapMods,clearLocks); };
        indicator "Caps Lock" { whichModState= locked; modifiers= Lock; };
        indicator "Num Lock" { whichModState= locked; modifiers= NumLock; };
    };
'''

def xkb_keymap(name, desc, keys, altgr):
    codes = sorted(set(KEYNAMES) | set(keys))
    out = ['xkb_keymap {', '    xkb_keycodes "pulsar" {', '        minimum = 8;', '        maximum = 255;']
    for c in codes:
        out.append(f'        <{KEYNAMES[c]}> = {c + 8};')
    out.append('        indicator 1 = "Caps Lock";')
    out.append('        indicator 2 = "Num Lock";')
    out.append('    };')
    out.append(XKB_TYPES + XKB_COMPAT)
    out.append(f'    xkb_symbols "pulsar-{name}" {{')
    out.append(f'        name[group1]= "{desc}";')
    for c in codes:
        kn = KEYNAMES[c]
        if c in keys:
            lv = keys[c]
            syms = [keysym(t) if t else 'NoSymbol' for t in lv]
            four = any(lv[2:])
            while len(syms) > 2 and syms[-1] == 'NoSymbol': syms.pop()
            if four:
                while len(syms) < 4: syms.append('NoSymbol')
                ty = 'FOUR_LEVEL_SEMIALPHABETIC' if c in ALPHA else 'FOUR_LEVEL'
            else:
                syms = syms[:2]
                ty = 'ALPHABETIC' if c in ALPHA else 'TWO_LEVEL'
            out.append(f'        key <{kn}> {{ type= "{ty}", [ {", ".join(syms)} ] }};')
        elif c == 100:
            sym = 'ISO_Level3_Shift' if altgr else 'Alt_R'
            out.append(f'        key <RALT> {{ type= "ONE_LEVEL", [ {sym} ] }};')
        elif c in KEYPAD:
            a, b = KEYPAD[c]
            out.append(f'        key <{kn}> {{ type= "KEYPAD", [ {a}, {b} ] }};')
        elif c == 83:
            dec = 'KP_Separator' if name.startswith('abnt') else 'KP_Decimal'
            out.append(f'        key <KPDL> {{ type= "KEYPAD", [ KP_Delete, {dec} ] }};')
        elif c in FIXED:
            f = FIXED[c]
            if isinstance(f, list):
                out.append(f'        key <{kn}> {{ type= "TWO_LEVEL", [ {", ".join(f)} ] }};')
            else:
                out.append(f'        key <{kn}> {{ type= "ONE_LEVEL", [ {f} ] }};')
    out.append('        modifier_map Shift { <LFSH>, <RTSH> };')
    out.append('        modifier_map Lock { <CAPS> };')
    out.append('        modifier_map Control { <LCTL>, <RCTL> };')
    out.append('        modifier_map Mod1 { <LALT>' + ('' if altgr else ', <RALT>') + ' };')
    out.append('        modifier_map Mod2 { <NMLK> };')
    out.append('        modifier_map Mod4 { <LWIN>, <RWIN> };')
    if altgr: out.append('        modifier_map Mod5 { <RALT> };')
    out.append('    };')
    out.append('};')
    return '\n'.join(out) + '\n'

# ---------------------------------------------------------------- Rust

def cp(tok):
    """codepoint do teclado nativo: tecla morta = 0x8000_0000 | acento isolado"""
    if not tok: return 0
    if tok in DEAD: return 0x80000000 | ord(DEAD[tok][1])
    return ord(tok)

def rust(compose):
    out = ['//! GERADO por tools/genkeymaps.py — nao edite a mao.',
           '//! Layouts do teclado nativo: por codigo evdev, os niveis [base, shift,',
           '//! altgr, altgr+shift]; 0 = nada; bit 31 = tecla morta (o resto e o acento).', '',
           'pub struct LayoutDef {', '    pub name: &\'static str,', '    pub altgr: bool,',
           '    pub keys: [[u32; 4]; 128],', '}', '']
    for name, (desc, keys, altgr) in LAYOUTS.items():
        ident = name.upper().replace('-', '_')
        out.append(f'pub static {ident}: LayoutDef = LayoutDef {{ name: "{name}", altgr: {str(altgr).lower()}, keys: {{')
        out.append('    let mut k = [[0u32; 4]; 128];')
        for c in sorted(keys):
            lv = [cp(t) for t in keys[c]]
            out.append(f'    k[{c}] = [{", ".join(hex(v) for v in lv)}];')
        out.append('    k')
        out.append('} };')
        out.append('')
    out.append('/// Teclas em que o Caps Lock age (letras e o c cedilha do ABNT).')
    out.append(f'pub const ALPHA: [u8; {len(ALPHA)}] = [{", ".join(str(c) for c in sorted(ALPHA))}];')
    out.append('')
    out.append('/// (acento da tecla morta, caractere, resultado)')
    out.append(f'pub const COMPOSE: [(u32, u32, u32); {len(compose)}] = [')
    for d, b, r in compose:
        bc = ord(DEAD[b][1]) if b in DEAD else ord(b)
        out.append(f'    ({hex(ord(DEAD[d][1]))}, {hex(bc)}, {hex(ord(r))}),')
    out.append('];')
    return '\n'.join(out) + '\n'

def xcompose(compose):
    out = ['# Compose do Pulsar OS (GERADO por tools/genkeymaps.py): teclas mortas.',
           '# Como no Brasil, acento agudo + c = c cedilha.', '']
    for d, b, r in compose:
        src = f'<{DEAD[b][0]}>' if b in DEAD else f'<{keysym(b) if b != " " else "space"}>'
        # o keysym do resultado e opcional: so quando ha um nome conhecido
        res = f' {NAMES[r]}' if r in NAMES else ''
        out.append(f'<{DEAD[d][0]}> {src} : "{r}"{res}')
    return '\n'.join(out) + '\n'

def main():
    comp = compose_table()
    with open(os.path.join(ROOT, 'user/plib/src/keymaps.rs'), 'w') as f: f.write(rust(comp))
    xdir = os.path.join(ROOT, 'rootfs/usr/share/pulsar/xkb')
    os.makedirs(xdir, exist_ok=True)
    have_xkbcomp = subprocess.run(['sh', '-c', 'command -v xkbcomp'], capture_output=True).returncode == 0
    for name, (desc, keys, altgr) in LAYOUTS.items():
        path = os.path.join(xdir, f'{name}.xkb')
        with open(path, 'w') as f: f.write(xkb_keymap(name, desc, keys, altgr))
        if have_xkbcomp:
            with tempfile.TemporaryDirectory() as td:
                r = subprocess.run(['xkbcomp', '-w', '0', '-xkb', path, os.path.join(td, 'out.xkb')],
                                   capture_output=True, text=True)
            if r.returncode != 0:
                sys.exit(f'[genkeymaps] {name}.xkb invalido segundo o xkbcomp:\n{r.stderr}')
    with open(os.path.join(ROOT, 'rootfs/usr/share/pulsar/Compose'), 'w') as f: f.write(xcompose(comp))
    print(f'[genkeymaps] {len(LAYOUTS)} layouts, {len(comp)} combinacoes de teclas mortas'
          + (' (XKB validado pelo xkbcomp)' if have_xkbcomp else ''))

if __name__ == '__main__':
    main()
