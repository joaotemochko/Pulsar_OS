//! ext2 — sistema de arquivos com diretorios, links simbolicos e arquivos
//! grandes. E o formato que aplicativos Linux esperam encontrar, e as
//! imagens sao geradas pelo `mke2fs -d` do host (ver Makefile) e podem ser
//! conferidas com `e2fsck`.
//!
//! Suporta revisao 1 com blocos de 1, 2 ou 4 KiB e as features `filetype`,
//! `sparse_super` e `large_file`. Volumes com qualquer outra feature
//! incompativel sao recusados (o Makefile cria o disco sem dir_index,
//! resize_inode e ext_attr).
//!
//! Tudo aqui supoe a trava FS presa (fs.rs). Os blocos passam por um cache
//! associativo (4 vias) com write-back; cada operacao publica termina com
//! `sync()`, entao o disco fica consistente entre chamadas.
//!
//! Erros sao numeros errno do Linux (positivos aqui; as syscalls negam).
use crate::blk;
use crate::frame_allocator;
use crate::uart::Uart;
use core::fmt::Write;

pub const ENOENT: i64 = 2;
pub const EIO: i64 = 5;
pub const EEXIST: i64 = 17;
pub const ENOTDIR: i64 = 20;
pub const EISDIR: i64 = 21;
pub const EINVAL: i64 = 22;
pub const EFBIG: i64 = 27;
pub const ENOSPC: i64 = 28;
pub const ENAMETOOLONG: i64 = 36;
pub const ENOTEMPTY: i64 = 39;
pub const ELOOP: i64 = 40;

pub type Res<T> = Result<T, i64>;

pub const ROOT_INO: u32 = 2;

// tipos (i_mode & S_IFMT)
pub const S_IFMT: u16 = 0o170000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFLNK: u16 = 0o120000;

// file_type das entradas de diretorio
pub const FT_REG: u8 = 1;
pub const FT_DIR: u8 = 2;
pub const FT_LNK: u8 = 7;

const MAGIC: u16 = 0xEF53;
const INCOMPAT_FILETYPE: u32 = 0x2;
const RO_SPARSE: u32 = 0x1;
const RO_LARGE_FILE: u32 = 0x2;

// ------------------------------------------------------------ cache

const WAYS: usize = 4;
const SETS: usize = 256;          // 1024 blocos (4 MiB com blocos de 4 KiB)
const SLOTS: usize = WAYS * SETS;

#[derive(Clone, Copy)]
struct Slot {
    blk: u32,
    valid: bool,
    dirty: bool,
    used: u32,   // relogio de uso (LRU dentro do conjunto)
}

static mut CACHE_MEM: u64 = 0;
static mut SLOT: [Slot; SLOTS] = [Slot { blk: 0, valid: false, dirty: false, used: 0 }; SLOTS];
static mut TICK: u32 = 0;

fn slots() -> &'static mut [Slot; SLOTS] {
    unsafe { &mut *(&raw mut SLOT) }
}

fn slot_ptr(i: usize) -> *mut u8 {
    unsafe { (CACHE_MEM + (i as u64) * fs().bs) as *mut u8 }
}

fn disk_io(write: bool, b: u32, pa: u64, nblocks: u32) -> bool {
    let f = fs();
    let spb = (f.bs / 512) as u32;
    let lba = b as u64 * spb as u64;
    if write { blk::write_sectors(lba, nblocks * spb, pa) } else { blk::read_sectors(lba, nblocks * spb, pa) }
}

/// Bloco `b` no cache. Com `load` = false (bloco recem-alocado), nao le o
/// disco: o conteudo vem zerado. O ponteiro so vale ate o proximo acesso ao
/// cache — copie o que precisar antes.
fn bget(b: u32, load: bool) -> Res<*mut u8> {
    let ss = slots();
    let set = (b as usize).wrapping_mul(0x9E37_79B9) >> 8 & (SETS - 1);
    let base = set * WAYS;
    unsafe { TICK = TICK.wrapping_add(1); }
    let now = unsafe { TICK };
    for i in base..base + WAYS {
        if ss[i].valid && ss[i].blk == b {
            ss[i].used = now;
            return Ok(slot_ptr(i));
        }
    }
    // vitima: invalida, ou a menos usada recentemente
    let mut v = base;
    for i in base..base + WAYS {
        if !ss[i].valid { v = i; break; }
        if now.wrapping_sub(ss[i].used) > now.wrapping_sub(ss[v].used) { v = i; }
    }
    if ss[v].valid && ss[v].dirty {
        // despejar um bloco sujo: grava todos de uma vez (em lote e em
        // ordem); os proximos despejos encontram blocos limpos
        if !flush_dirty() { return Err(EIO); }
    }
    ss[v] = Slot { blk: b, valid: false, dirty: false, used: now };
    if load {
        if !disk_io(false, b, slot_ptr(v) as u64, 1) { return Err(EIO); }
    } else {
        unsafe { core::ptr::write_bytes(slot_ptr(v), 0, fs().bs as usize) };
        ss[v].dirty = true;
    }
    ss[v].valid = true;
    Ok(slot_ptr(v))
}

/// Marca o bloco `b` (que acabou de ser obtido com bget) como modificado.
fn bdirty(b: u32) {
    let ss = slots();
    let set = (b as usize).wrapping_mul(0x9E37_79B9) >> 8 & (SETS - 1);
    for i in set * WAYS..set * WAYS + WAYS {
        if ss[i].valid && ss[i].blk == b { ss[i].dirty = true; return; }
    }
}

/// Esquece o bloco `b` (liberado): nao sera escrito de volta.
fn bforget(b: u32) {
    let ss = slots();
    let set = (b as usize).wrapping_mul(0x9E37_79B9) >> 8 & (SETS - 1);
    for i in set * WAYS..set * WAYS + WAYS {
        if ss[i].valid && ss[i].blk == b { ss[i].valid = false; ss[i].dirty = false; }
    }
}

/// Grava todos os blocos sujos, ordenados por numero; blocos consecutivos
/// viram uma so requisicao scatter-gather.
fn flush_dirty() -> bool {
    let mut list = [(0u32, 0u16); SLOTS];
    let mut n = 0;
    for i in 0..SLOTS {
        let s = slots()[i];
        if s.valid && s.dirty { list[n] = (s.blk, i as u16); n += 1; }
    }
    if n == 0 { return true; }
    let list = &mut list[..n];
    list.sort_unstable_by_key(|e| e.0);
    let bs = fs().bs as u32;
    let mut ok = true;
    let mut i = 0;
    while i < n {
        let mut segs = [(0u64, 0u32); blk::MAX_SEGS];
        let mut k = 0;
        while i + k < n && k < blk::MAX_SEGS && list[i + k].0 == list[i].0 + k as u32 {
            segs[k] = (slot_ptr(list[i + k].1 as usize) as u64, bs);
            k += 1;
        }
        let lba = list[i].0 as u64 * (bs as u64 / 512);
        if blk::transfer_sg(true, lba, &segs[..k]) {
            for j in i..i + k { slots()[list[j].1 as usize].dirty = false; }
        } else {
            ok = false;
        }
        i += k;
    }
    ok
}

/// Le adiantado os blocos `b`, `b`+1, ... (ate `n`) que faltam no cache,
/// numa requisicao so. Para no primeiro que ja esta no cache.
fn prefetch(b: u32, n: usize) {
    let ss = slots();
    unsafe { TICK = TICK.wrapping_add(1); }
    let now = unsafe { TICK };
    let mut segs = [(0u64, 0u32); blk::MAX_SEGS];
    let mut taken = [0usize; blk::MAX_SEGS];
    let mut k = 0;
    while k < n.min(blk::MAX_SEGS) {
        let blkno = b + k as u32;
        let set = (blkno as usize).wrapping_mul(0x9E37_79B9) >> 8 & (SETS - 1);
        let base = set * WAYS;
        if (base..base + WAYS).any(|i| ss[i].valid && ss[i].blk == blkno) { break; }
        // vitima limpa que nao seja desta mesma leitura
        let Some(v) = (base..base + WAYS)
            .filter(|&i| !(ss[i].valid && (ss[i].dirty || ss[i].used == now)))
            .min_by_key(|&i| if ss[i].valid { ss[i].used.wrapping_sub(now) } else { 0 })
        else { break };
        ss[v] = Slot { blk: blkno, valid: false, dirty: false, used: now };
        segs[k] = (slot_ptr(v) as u64, fs().bs as u32);
        taken[k] = v;
        k += 1;
    }
    if k == 0 { return; }
    let ok = blk::transfer_sg(false, b as u64 * (fs().bs / 512), &segs[..k]);
    for j in 0..k {
        ss[taken[j]].valid = ok;
    }
}

/// Grava no disco todos os blocos modificados (e o superbloco, se os
/// contadores de livres mudaram).
pub fn sync() -> bool {
    let mut ok = true;
    let f = fs();
    if f.mounted && unsafe { SB_COUNTS } != (f.free_blocks, f.free_inodes) {
        ok = flush_super();
        unsafe { SB_COUNTS = (f.free_blocks, f.free_inodes); }
    }
    flush_dirty() && ok
}

fn cached(b: u32) -> bool {
    let ss = slots();
    let set = (b as usize).wrapping_mul(0x9E37_79B9) >> 8 & (SETS - 1);
    (set * WAYS..set * WAYS + WAYS).any(|i| ss[i].valid && ss[i].blk == b)
}

fn rd32(b: u32, off: usize) -> Res<u32> {
    let p = bget(b, true)?;
    Ok(unsafe { core::ptr::read_unaligned(p.add(off) as *const u32) })
}

fn wr32(b: u32, off: usize, v: u32) -> Res<()> {
    let p = bget(b, true)?;
    unsafe { core::ptr::write_unaligned(p.add(off) as *mut u32, v) };
    bdirty(b);
    Ok(())
}

// ------------------------------------------------------------ superbloco

struct Fs {
    bs: u64,             // tamanho do bloco
    ppb: u64,            // ponteiros por bloco
    blocks: u32,
    inodes: u32,
    bpg: u32,
    ipg: u32,
    groups: u32,
    first_data: u32,
    inode_size: u32,
    gdt_block: u32,
    free_blocks: u32,
    free_inodes: u32,
    mounted: bool,
}

static mut FS_STATE: Fs = Fs {
    bs: 1024, ppb: 256, blocks: 0, inodes: 0, bpg: 0, ipg: 0, groups: 0, first_data: 0,
    inode_size: 128, gdt_block: 0, free_blocks: 0, free_inodes: 0, mounted: false,
};

fn fs() -> &'static mut Fs {
    unsafe { &mut *(&raw mut FS_STATE) }
}

#[repr(C, align(512))]
struct SbBuf([u8; 1024]);
static mut SB: SbBuf = SbBuf([0; 1024]);
/// Contadores de livres que estao gravados no superbloco em disco.
static mut SB_COUNTS: (u32, u32) = (0, 0);

fn sb_u32(off: usize) -> u32 { unsafe { u32::from_le_bytes(SB.0[off..off + 4].try_into().unwrap()) } }
fn sb_u16(off: usize) -> u16 { unsafe { u16::from_le_bytes(SB.0[off..off + 2].try_into().unwrap()) } }

/// Le o superbloco (bytes 1024..2048 do disco) e prepara o cache.
pub fn mount() -> bool {
    let mut serial = Uart;
    let pa = unsafe { &raw mut SB.0 as *mut u8 as u64 };
    if !blk::read_sectors(2, 2, pa) || sb_u16(56) != MAGIC { return false; }
    let rev = sb_u32(76);
    let incompat = if rev >= 1 { sb_u32(96) } else { 0 };
    let ro = if rev >= 1 { sb_u32(100) } else { 0 };
    if incompat & !INCOMPAT_FILETYPE != 0 || ro & !(RO_SPARSE | RO_LARGE_FILE) != 0 {
        let _ = write!(serial, "[ext2] features nao suportadas: incompat {:#x} ro {:#x}\n", incompat, ro);
        return false;
    }
    let f = fs();
    f.bs = 1024u64 << sb_u32(24);
    if f.bs > 4096 { return false; }
    f.ppb = f.bs / 4;
    f.inodes = sb_u32(0);
    f.blocks = sb_u32(4);
    f.free_blocks = sb_u32(12);
    f.free_inodes = sb_u32(16);
    f.first_data = sb_u32(20);
    f.bpg = sb_u32(32);
    f.ipg = sb_u32(40);
    f.inode_size = if rev >= 1 { sb_u16(88) as u32 } else { 128 };
    f.groups = (f.blocks - f.first_data).div_ceil(f.bpg);
    f.gdt_block = f.first_data + 1;
    unsafe { SB_COUNTS = (f.free_blocks, f.free_inodes); }
    let pages = (SLOTS as u64 * f.bs).div_ceil(4096) as usize;
    let Some(mem) = frame_allocator::alloc_contig(pages) else { return false };
    unsafe { CACHE_MEM = mem; }
    f.mounted = true;
    let label_end = unsafe { SB.0[120..136].iter().position(|&c| c == 0).unwrap_or(16) };
    let label = unsafe { core::str::from_utf8(&SB.0[120..120 + label_end]).unwrap_or("?") };
    let _ = write!(serial, "[ext2] volume '{}': {} MiB, blocos de {} B, {} grupos, {} inodes livres, {} MiB livres\n",
        label, f.blocks as u64 * f.bs >> 20, f.bs, f.groups, f.free_inodes, f.free_blocks as u64 * f.bs >> 20);
    true
}

/// Atualiza contadores e horario de escrita no superbloco em disco.
fn flush_super() -> bool {
    let f = fs();
    unsafe {
        SB.0[12..16].copy_from_slice(&f.free_blocks.to_le_bytes());
        SB.0[16..20].copy_from_slice(&f.free_inodes.to_le_bytes());
        SB.0[48..52].copy_from_slice(&now().to_le_bytes());
        blk::write_sectors(2, 2, &raw const SB.0 as *const u8 as u64)
    }
}

pub fn stats() -> (u64, u64, u64, u64, u64) {
    let f = fs();
    (f.bs, f.blocks as u64, f.free_blocks as u64, f.inodes as u64, f.free_inodes as u64)
}

fn now() -> u32 {
    crate::arch::rtc_seconds() as u32
}

// ------------------------------------------------------------ grupos

fn gd_off(g: u32) -> (u32, usize) {
    let per = (fs().bs / 32) as u32;
    (fs().gdt_block + g / per, ((g % per) * 32) as usize)
}

fn gd_u32(g: u32, field: usize) -> Res<u32> {
    let (b, o) = gd_off(g);
    rd32(b, o + field)
}

fn gd_add16(g: u32, field: usize, delta: i32) -> Res<()> {
    let (b, o) = gd_off(g);
    let p = bget(b, true)?;
    unsafe {
        let q = p.add(o + field) as *mut u16;
        let v = core::ptr::read_unaligned(q) as i32 + delta;
        core::ptr::write_unaligned(q, v.max(0) as u16);
    }
    bdirty(b);
    Ok(())
}

const GD_BLOCK_BITMAP: usize = 0;
const GD_INODE_BITMAP: usize = 4;
const GD_INODE_TABLE: usize = 8;
const GD_FREE_BLOCKS: usize = 12;
const GD_FREE_INODES: usize = 14;
const GD_USED_DIRS: usize = 16;

/// Procura um bit zero no bitmap `bm` (ate `limit` bits), marca e devolve.
fn bitmap_take(bm: u32, limit: u32) -> Res<Option<u32>> {
    let p = bget(bm, true)?;
    let words = limit.div_ceil(64) as usize;
    for w in 0..words {
        let v = unsafe { core::ptr::read_unaligned((p as *const u64).add(w)) };
        if v == u64::MAX { continue; }
        let bit = (!v).trailing_zeros();
        let idx = w as u32 * 64 + bit;
        if idx >= limit { break; }
        unsafe { core::ptr::write_unaligned((p as *mut u64).add(w), v | (1u64 << bit)) };
        bdirty(bm);
        return Ok(Some(idx));
    }
    Ok(None)
}

fn bitmap_clear(bm: u32, idx: u32) -> Res<bool> {
    let p = bget(bm, true)?;
    let byte = unsafe { p.add((idx / 8) as usize) };
    let mask = 1u8 << (idx % 8);
    let was = unsafe { *byte & mask != 0 };
    unsafe { *byte &= !mask };
    bdirty(bm);
    Ok(was)
}

/// Aloca um bloco (zerado no cache), de preferencia no grupo `goal`.
fn alloc_block(goal: u32) -> Res<u32> {
    let f = fs();
    if f.free_blocks == 0 { return Err(ENOSPC); }
    for k in 0..f.groups {
        let g = (goal + k) % f.groups;
        if gd_u32(g, GD_FREE_BLOCKS)? & 0xFFFF == 0 { continue; }
        let bm = gd_u32(g, GD_BLOCK_BITMAP)?;
        let in_group = (f.blocks - f.first_data - g * f.bpg).min(f.bpg);
        if let Some(i) = bitmap_take(bm, in_group)? {
            gd_add16(g, GD_FREE_BLOCKS, -1)?;
            f.free_blocks -= 1;
            let b = f.first_data + g * f.bpg + i;
            bget(b, false)?;
            return Ok(b);
        }
    }
    Err(ENOSPC)
}

fn free_block(b: u32) -> Res<()> {
    let f = fs();
    if b < f.first_data || b >= f.blocks { return Ok(()); }
    let g = (b - f.first_data) / f.bpg;
    let bm = gd_u32(g, GD_BLOCK_BITMAP)?;
    if bitmap_clear(bm, (b - f.first_data) % f.bpg)? {
        gd_add16(g, GD_FREE_BLOCKS, 1)?;
        f.free_blocks += 1;
    }
    bforget(b);
    Ok(())
}

fn alloc_inode(dir: bool, goal: u32) -> Res<u32> {
    let f = fs();
    if f.free_inodes == 0 { return Err(ENOSPC); }
    for k in 0..f.groups {
        let g = (goal + k) % f.groups;
        if gd_u32(g, GD_FREE_INODES)? & 0xFFFF == 0 { continue; }
        let bm = gd_u32(g, GD_INODE_BITMAP)?;
        if let Some(i) = bitmap_take(bm, f.ipg)? {
            gd_add16(g, GD_FREE_INODES, -1)?;
            if dir { gd_add16(g, GD_USED_DIRS, 1)?; }
            f.free_inodes -= 1;
            return Ok(g * f.ipg + i + 1);
        }
    }
    Err(ENOSPC)
}

fn free_inode(ino: u32, dir: bool) -> Res<()> {
    let f = fs();
    let g = (ino - 1) / f.ipg;
    let bm = gd_u32(g, GD_INODE_BITMAP)?;
    if bitmap_clear(bm, (ino - 1) % f.ipg)? {
        gd_add16(g, GD_FREE_INODES, 1)?;
        if dir { gd_add16(g, GD_USED_DIRS, -1)?; }
        f.free_inodes += 1;
    }
    Ok(())
}

// ------------------------------------------------------------ inodes

/// Os primeiros 128 bytes de um inode (o resto, se houver, e preservado).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inode {
    pub mode: u16,
    pub uid: u16,
    pub size_lo: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links: u16,
    pub blocks512: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15],
    pub generation: u32,
    pub file_acl: u32,
    pub size_hi: u32,
    pub faddr: u32,
    pub osd2: [u8; 12],
}

impl Inode {
    fn zeroed() -> Self { unsafe { core::mem::zeroed() } }
    pub fn size(&self) -> u64 {
        if self.mode & S_IFMT == S_IFREG { self.size_lo as u64 | (self.size_hi as u64) << 32 } else { self.size_lo as u64 }
    }
    fn set_size(&mut self, s: u64) {
        self.size_lo = s as u32;
        if self.mode & S_IFMT == S_IFREG { self.size_hi = (s >> 32) as u32; }
    }
    pub fn kind(&self) -> u16 { self.mode & S_IFMT }
    pub fn is_dir(&self) -> bool { self.kind() == S_IFDIR }
    /// Link simbolico "rapido": o alvo mora dentro de block[] (< 60 bytes).
    fn fast_symlink(&self) -> bool { self.kind() == S_IFLNK && self.blocks512 == 0 }
}

fn inode_loc(ino: u32) -> Res<(u32, usize)> {
    let f = fs();
    if ino == 0 || ino > f.inodes { return Err(EINVAL); }
    let g = (ino - 1) / f.ipg;
    let idx = (ino - 1) % f.ipg;
    let table = gd_u32(g, GD_INODE_TABLE)?;
    let byte = idx as u64 * f.inode_size as u64;
    Ok((table + (byte / f.bs) as u32, (byte % f.bs) as usize))
}

pub fn read_inode(ino: u32) -> Res<Inode> {
    let (b, o) = inode_loc(ino)?;
    let p = bget(b, true)?;
    Ok(unsafe { core::ptr::read_unaligned(p.add(o) as *const Inode) })
}

fn write_inode(ino: u32, n: &Inode) -> Res<()> {
    let (b, o) = inode_loc(ino)?;
    let p = bget(b, true)?;
    unsafe { core::ptr::write_unaligned(p.add(o) as *mut Inode, *n) };
    bdirty(b);
    Ok(())
}

fn group_of(ino: u32) -> u32 { (ino - 1) / fs().ipg }

// ------------------------------------------------------------ mapa de blocos

/// Caminho ate o bloco logico `l`: nivel (0 direto, 1..3 indireto) e
/// indices em cada nivel.
fn block_path(l: u64) -> Option<(usize, [usize; 4])> {
    let p = fs().ppb;
    if l < 12 { return Some((0, [l as usize, 0, 0, 0])); }
    let l = l - 12;
    if l < p { return Some((1, [12, l as usize, 0, 0])); }
    let l = l - p;
    if l < p * p { return Some((2, [13, (l / p) as usize, (l % p) as usize, 0])); }
    let l = l - p * p;
    if l < p * p * p {
        return Some((3, [14, (l / (p * p)) as usize, (l / p % p) as usize, (l % p) as usize]));
    }
    None
}

/// Bloco fisico do bloco logico `l` (0 = buraco).
fn bmap(n: &Inode, l: u64) -> Res<u32> {
    let Some((depth, idx)) = block_path(l) else { return Ok(0) };
    let mut b = n.block[idx[0]];
    for k in 1..=depth {
        if b == 0 { return Ok(0); }
        b = rd32(b, idx[k] * 4)?;
    }
    Ok(b)
}

/// Como bmap, mas aloca o bloco (e os indiretos) se faltar.
fn bmap_alloc(ino: u32, n: &mut Inode, l: u64) -> Res<u32> {
    let Some((depth, idx)) = block_path(l) else { return Err(EFBIG) };
    let goal = group_of(ino);
    let per = (fs().bs / 512) as u32;
    if n.block[idx[0]] == 0 {
        n.block[idx[0]] = alloc_block(goal)?;
        n.blocks512 += per;
    }
    let mut b = n.block[idx[0]];
    for k in 1..=depth {
        let mut next = rd32(b, idx[k] * 4)?;
        if next == 0 {
            next = alloc_block(goal)?;
            n.blocks512 += per;
            wr32(b, idx[k] * 4, next)?;
        }
        b = next;
    }
    Ok(b)
}

/// Libera, dentro do indireto `b` (nivel `level`, cobrindo a partir do
/// bloco logico `base`), tudo que estiver em >= `keep`. Retorna true se o
/// indireto ficou vazio (o chamador o libera).
fn trunc_ind(n: &mut Inode, b: u32, level: u32, base: u64, keep: u64) -> Res<bool> {
    let p = fs().ppb;
    let span = p.pow(level - 1);
    let per = (fs().bs / 512) as u32;
    let mut empty = true;
    for i in 0..p {
        let child = rd32(b, (i * 4) as usize)?;
        if child == 0 { continue; }
        let cbase = base + i * span;
        if cbase + span <= keep { empty = false; continue; }
        let drop = if level == 1 { cbase >= keep } else { trunc_ind(n, child, level - 1, cbase, keep)? };
        if drop {
            free_block(child)?;
            n.blocks512 = n.blocks512.saturating_sub(per);
            wr32(b, (i * 4) as usize, 0)?;
        } else {
            empty = false;
        }
    }
    Ok(empty)
}

/// Encolhe (ou estica, com buraco) o arquivo para `size` bytes.
fn truncate_inode(n: &mut Inode, size: u64) -> Res<()> {
    let bs = fs().bs;
    if size < n.size() && !n.fast_symlink() {
        let keep = size.div_ceil(bs);
        let per = (bs / 512) as u32;
        for i in 0..12 {
            if i as u64 >= keep && n.block[i] != 0 {
                free_block(n.block[i])?;
                n.blocks512 = n.blocks512.saturating_sub(per);
                n.block[i] = 0;
            }
        }
        let p = fs().ppb;
        let bases = [12u64, 12 + p, 12 + p + p * p];
        for lvl in 1..=3u32 {
            let slot = 11 + lvl as usize;
            if n.block[slot] == 0 { continue; }
            if trunc_ind(n, n.block[slot], lvl, bases[lvl as usize - 1], keep)? {
                free_block(n.block[slot])?;
                n.blocks512 = n.blocks512.saturating_sub(per);
                n.block[slot] = 0;
            }
        }
        // zera o fim do ultimo bloco mantido, para um crescimento futuro
        // nao expor dados antigos
        if size % bs != 0 {
            let b = bmap(n, size / bs)?;
            if b != 0 {
                let ptr = bget(b, true)?;
                let off = (size % bs) as usize;
                unsafe { core::ptr::write_bytes(ptr.add(off), 0, bs as usize - off) };
                bdirty(b);
            }
        }
    }
    n.set_size(size);
    Ok(())
}

// ------------------------------------------------------------ dados

/// Le ate `len` bytes do inode a partir de `off` para `dst` (memoria ja
/// validada). Retorna os bytes lidos.
pub fn read(ino: u32, off: u64, dst: *mut u8, len: usize) -> Res<usize> {
    let n = read_inode(ino)?;
    let size = n.size();
    if off >= size { return Ok(0); }
    let len = (len as u64).min(size - off) as usize;
    let bs = fs().bs;
    let mut done = 0usize;
    while done < len {
        let pos = off + done as u64;
        let inblk = (pos % bs) as usize;
        let chunk = (bs as usize - inblk).min(len - done);
        let b = bmap(&n, pos / bs)?;
        if b != 0 && !cached(b) {
            // quantos blocos seguintes do arquivo sao contiguos no disco
            let want = ((len - done) as u64).div_ceil(bs).min(32) as u32;
            let mut run = 1u32;
            while run < want && bmap(&n, pos / bs + run as u64)? == b + run { run += 1; }
            prefetch(b, run as usize);
        }
        unsafe {
            if b == 0 {
                core::ptr::write_bytes(dst.add(done), 0, chunk);
            } else {
                let p = bget(b, true)?;
                core::ptr::copy_nonoverlapping(p.add(inblk), dst.add(done), chunk);
            }
        }
        done += chunk;
    }
    Ok(len)
}

/// Le o arquivo inteiro para um buffer FISICAMENTE contiguo (loader):
/// blocos vizinhos no disco viram uma unica requisicao, direto no destino.
pub fn read_all_phys(ino: u32, dst_pa: u64) -> Res<u64> {
    let n = read_inode(ino)?;
    let size = n.size();
    let bs = fs().bs;
    let nblk = size.div_ceil(bs);
    let mut l = 0u64;
    while l < nblk {
        let b = bmap(&n, l)?;
        if b == 0 {
            unsafe { core::ptr::write_bytes((dst_pa + l * bs) as *mut u8, 0, bs as usize) };
            l += 1;
            continue;
        }
        // estende a corrida enquanto os blocos forem consecutivos (ate 1 MiB)
        let mut run = 1u64;
        while l + run < nblk && run * bs < (1 << 20) && bmap(&n, l + run)? == b + run as u32 {
            run += 1;
        }
        // o cache pode ter versoes mais novas que o disco
        sync();
        if !disk_io(false, b, dst_pa + l * bs, run as u32) { return Err(EIO); }
        l += run;
    }
    Ok(size)
}

/// Escreve `len` bytes de `src` em `off`, crescendo o arquivo se preciso.
pub fn write(ino: u32, off: u64, src: *const u8, len: usize) -> Res<usize> {
    let mut n = read_inode(ino)?;
    if n.is_dir() { return Err(EISDIR); }
    let bs = fs().bs;
    off.checked_add(len as u64).ok_or(EFBIG)?;
    let mut done = 0usize;
    let mut err = None;
    while done < len {
        let pos = off + done as u64;
        let inblk = (pos % bs) as usize;
        let chunk = (bs as usize - inblk).min(len - done);
        let b = match bmap_alloc(ino, &mut n, pos / bs) {
            Ok(b) => b,
            Err(e) => { err = Some(e); break; }
        };
        // bloco inteiro sobrescrito nao precisa ser lido do disco
        let p = match bget(b, !(inblk == 0 && chunk == bs as usize)) {
            Ok(p) => p,
            Err(e) => { err = Some(e); break; }
        };
        unsafe { core::ptr::copy_nonoverlapping(src.add(done), p.add(inblk), chunk) };
        bdirty(b);
        done += chunk;
    }
    let new_end = off + done as u64;
    if new_end > n.size() { n.set_size(new_end); }
    let t = now();
    n.mtime = t;
    n.ctime = t;
    write_inode(ino, &n)?;
    sync();
    match err {
        Some(e) if done == 0 => Err(e),
        _ => Ok(done),
    }
}

pub fn truncate(ino: u32, size: u64) -> Res<()> {
    let mut n = read_inode(ino)?;
    if n.is_dir() { return Err(EISDIR); }
    truncate_inode(&mut n, size)?;
    let t = now();
    n.mtime = t;
    n.ctime = t;
    write_inode(ino, &n)?;
    sync();
    Ok(())
}

// ------------------------------------------------------------ diretorios

/// Entrada de diretorio decodificada.
#[derive(Clone, Copy)]
pub struct Dirent {
    pub ino: u32,
    pub ftype: u8,
    pub name: [u8; 255],
    pub name_len: usize,
    pub next: u64,    // posicao (byte) da entrada seguinte
}

impl Dirent {
    pub fn name(&self) -> &[u8] { &self.name[..self.name_len] }
}

/// Le a proxima entrada viva a partir da posicao `pos` (bytes).
pub fn readdir(dir: u32, pos: u64) -> Res<Option<Dirent>> {
    let n = read_inode(dir)?;
    if !n.is_dir() { return Err(ENOTDIR); }
    let bs = fs().bs;
    let size = n.size();
    let mut pos = pos;
    while pos < size {
        let b = bmap(&n, pos / bs)?;
        let off = (pos % bs) as usize;
        if b == 0 { pos = (pos / bs + 1) * bs; continue; }
        let p = bget(b, true)?;
        let (ino, rec, nlen, ft) = unsafe {
            (core::ptr::read_unaligned(p.add(off) as *const u32),
             core::ptr::read_unaligned(p.add(off + 4) as *const u16) as usize,
             *p.add(off + 6) as usize, *p.add(off + 7))
        };
        if rec < 8 || off + rec > bs as usize { return Err(EIO); }
        let next = pos + rec as u64;
        if ino != 0 && nlen > 0 {
            let mut d = Dirent { ino, ftype: ft, name: [0; 255], name_len: nlen, next };
            unsafe { core::ptr::copy_nonoverlapping(p.add(off + 8), d.name.as_mut_ptr(), nlen) };
            return Ok(Some(d));
        }
        pos = next;
    }
    Ok(None)
}

pub fn lookup(dir: u32, name: &[u8]) -> Res<Option<(u32, u8)>> {
    let mut pos = 0;
    while let Some(d) = readdir(dir, pos)? {
        if d.name() == name { return Ok(Some((d.ino, d.ftype))); }
        pos = d.next;
    }
    Ok(None)
}

fn rec_need(nlen: usize) -> usize { (8 + nlen + 3) & !3 }

/// Insere `name` -> `ino` no diretorio (em espaco livre ou num bloco novo).
fn dir_add(dir: u32, name: &[u8], ino: u32, ft: u8) -> Res<()> {
    let mut n = read_inode(dir)?;
    let bs = fs().bs;
    let need = rec_need(name.len());
    let nblk = n.size() / bs;
    for l in 0..nblk {
        let b = bmap(&n, l)?;
        if b == 0 { continue; }
        let p = bget(b, true)?;
        let mut off = 0usize;
        while off < bs as usize {
            let (eino, rec, nlen) = unsafe {
                (core::ptr::read_unaligned(p.add(off) as *const u32),
                 core::ptr::read_unaligned(p.add(off + 4) as *const u16) as usize,
                 *p.add(off + 6) as usize)
            };
            if rec < 8 { return Err(EIO); }
            let used = if eino == 0 { 0 } else { rec_need(nlen) };
            if rec - used >= need {
                unsafe {
                    let at = if eino == 0 {
                        off
                    } else {
                        core::ptr::write_unaligned(p.add(off + 4) as *mut u16, used as u16);
                        off + used
                    };
                    let new_rec = rec - (at - off);
                    write_dirent(p.add(at), ino, new_rec, name, ft);
                }
                bdirty(b);
                return touch_dir(dir);
            }
            off += rec;
        }
    }
    // sem espaco: bloco novo no fim
    let b = bmap_alloc(dir, &mut n, nblk)?;
    let p = bget(b, true)?;
    unsafe { write_dirent(p, ino, bs as usize, name, ft) };
    bdirty(b);
    n.set_size((nblk + 1) * bs);
    let t = now();
    n.mtime = t;
    n.ctime = t;
    write_inode(dir, &n)
}

unsafe fn write_dirent(p: *mut u8, ino: u32, rec: usize, name: &[u8], ft: u8) {
    unsafe {
        core::ptr::write_unaligned(p as *mut u32, ino);
        core::ptr::write_unaligned(p.add(4) as *mut u16, rec as u16);
        *p.add(6) = name.len() as u8;
        *p.add(7) = ft;
        core::ptr::copy_nonoverlapping(name.as_ptr(), p.add(8), name.len());
    }
}

fn touch_dir(dir: u32) -> Res<()> {
    let mut n = read_inode(dir)?;
    let t = now();
    n.mtime = t;
    n.ctime = t;
    write_inode(dir, &n)
}

/// Remove a entrada `name` (funde o espaco com a anterior do bloco).
fn dir_remove(dir: u32, name: &[u8]) -> Res<()> {
    let n = read_inode(dir)?;
    let bs = fs().bs;
    for l in 0..n.size() / bs {
        let b = bmap(&n, l)?;
        if b == 0 { continue; }
        let p = bget(b, true)?;
        let mut off = 0usize;
        let mut prev: Option<usize> = None;
        while off < bs as usize {
            let (eino, rec, nlen) = unsafe {
                (core::ptr::read_unaligned(p.add(off) as *const u32),
                 core::ptr::read_unaligned(p.add(off + 4) as *const u16) as usize,
                 *p.add(off + 6) as usize)
            };
            if rec < 8 { return Err(EIO); }
            let matches = eino != 0 && nlen == name.len()
                && unsafe { core::slice::from_raw_parts(p.add(off + 8), nlen) } == name;
            if matches {
                unsafe {
                    match prev {
                        Some(po) => {
                            let prec = core::ptr::read_unaligned(p.add(po + 4) as *const u16) as usize;
                            core::ptr::write_unaligned(p.add(po + 4) as *mut u16, (prec + rec) as u16);
                        }
                        None => core::ptr::write_unaligned(p as *mut u32, 0),
                    }
                }
                bdirty(b);
                return touch_dir(dir);
            }
            prev = Some(off);
            off += rec;
        }
    }
    Err(ENOENT)
}

fn dir_is_empty(dir: u32) -> Res<bool> {
    let mut pos = 0;
    while let Some(d) = readdir(dir, pos)? {
        if d.name() != b"." && d.name() != b".." { return Ok(false); }
        pos = d.next;
    }
    Ok(true)
}

/// Troca o alvo de ".." de um diretorio movido.
fn set_dotdot(dir: u32, parent: u32) -> Res<()> {
    let n = read_inode(dir)?;
    let b = bmap(&n, 0)?;
    if b == 0 { return Err(EIO); }
    let p = bget(b, true)?;
    let rec = unsafe { core::ptr::read_unaligned(p.add(4) as *const u16) } as usize;
    unsafe { core::ptr::write_unaligned(p.add(rec) as *mut u32, parent) };
    bdirty(b);
    Ok(())
}

// ------------------------------------------------------------ caminhos

const MAX_LINKS: usize = 16;
pub const PATH_MAX: usize = 1024;

pub fn readlink(ino: u32, out: &mut [u8]) -> Res<usize> {
    let n = read_inode(ino)?;
    if n.kind() != S_IFLNK { return Err(EINVAL); }
    let len = (n.size() as usize).min(out.len());
    if n.fast_symlink() {
        let raw = unsafe { core::slice::from_raw_parts(n.block.as_ptr() as *const u8, 60) };
        out[..len].copy_from_slice(&raw[..len]);
        Ok(len)
    } else {
        read(ino, 0, out.as_mut_ptr(), len)
    }
}

/// Resolve um caminho; relativos partem do diretorio `start`. Com
/// `follow` = false, o ultimo componente nao e seguido se for link
/// simbolico (lstat, unlink, readlink).
pub fn resolve_at(start: u32, path: &[u8], follow: bool) -> Res<u32> {
    let mut buf = [0u8; PATH_MAX];
    if path.is_empty() { return Err(ENOENT); }
    if path.len() > PATH_MAX { return Err(ENAMETOOLONG); }
    buf[..path.len()].copy_from_slice(path);
    let mut len = path.len();
    let mut links = 0;
    'restart: loop {
        let mut cur = if buf[0] == b'/' { ROOT_INO } else { start };
        let mut i = 0;
        while i < len {
            while i < len && buf[i] == b'/' { i += 1; }
            if i >= len { break; }
            let s = i;
            while i < len && buf[i] != b'/' { i += 1; }
            let comp = &buf[s..i];
            if comp.len() > 255 { return Err(ENAMETOOLONG); }
            let dirn = read_inode(cur)?;
            if !dirn.is_dir() { return Err(ENOTDIR); }
            if comp == b"." { continue; }
            let Some((ino, _)) = lookup(cur, comp)? else { return Err(ENOENT) };
            let last = buf[i..len].iter().all(|&c| c == b'/');
            let n = read_inode(ino)?;
            if n.kind() == S_IFLNK && (follow || !last) {
                links += 1;
                if links > MAX_LINKS { return Err(ELOOP); }
                // novo caminho = alvo + resto; alvo relativo parte do dir atual
                let mut target = [0u8; PATH_MAX];
                let tl = readlink(ino, &mut target)?;
                let rest_len = len - i;
                let mut nb = [0u8; PATH_MAX];
                let mut k = 0;
                if target.first() != Some(&b'/') {
                    // prefixo: o que ja foi percorrido ate o componente
                    if s > PATH_MAX { return Err(ENAMETOOLONG); }
                    nb[..s].copy_from_slice(&buf[..s]);
                    k = s;
                }
                if k + tl + rest_len > PATH_MAX { return Err(ENAMETOOLONG); }
                nb[k..k + tl].copy_from_slice(&target[..tl]);
                k += tl;
                nb[k..k + rest_len].copy_from_slice(&buf[i..len]);
                k += rest_len;
                buf = nb;
                len = k;
                continue 'restart;
            }
            cur = ino;
        }
        return Ok(cur);
    }
}

/// Divide em (diretorio pai resolvido, nome final); relativo parte de `at`.
fn parent_of(at: u32, path: &[u8]) -> Res<(u32, &[u8])> {
    let mut end = path.len();
    while end > 1 && path[end - 1] == b'/' { end -= 1; }
    let p = &path[..end];
    if p.is_empty() { return Err(ENOENT); }
    let (dir, name) = match p.iter().rposition(|&c| c == b'/') {
        Some(slash) => (resolve_at(at, &p[..slash.max(1)], true)?, &p[slash + 1..]),
        None => (at, p),
    };
    if name.is_empty() || name == b"." || name == b".." { return Err(EINVAL); }
    if name.len() > 255 { return Err(ENAMETOOLONG); }
    if !read_inode(dir)?.is_dir() { return Err(ENOTDIR); }
    Ok((dir, name))
}

fn new_inode(mode: u16, dir_hint: u32, is_dir: bool) -> Res<(u32, Inode)> {
    let ino = alloc_inode(is_dir, group_of(dir_hint))?;
    let t = now();
    let mut n = Inode::zeroed();
    n.mode = mode;
    n.atime = t;
    n.ctime = t;
    n.mtime = t;
    n.links = 1;
    // inodes maiores que 128 bytes: zera a parte extra (i_extra_isize = 0)
    let (b, o) = inode_loc(ino)?;
    let p = bget(b, true)?;
    unsafe { core::ptr::write_bytes(p.add(o), 0, fs().inode_size as usize) };
    bdirty(b);
    Ok((ino, n))
}

/// Cria um arquivo regular. Retorna (inode, criado agora?).
pub fn create(at: u32, path: &[u8], perm: u16, excl: bool) -> Res<(u32, bool)> {
    let (dir, name) = parent_of(at, path)?;
    if let Some((ino, _)) = lookup(dir, name)? {
        if excl { return Err(EEXIST); }
        let n = read_inode(ino)?;
        if n.kind() == S_IFLNK { return Ok((resolve_at(at, path, true)?, false)); }
        return Ok((ino, false));
    }
    let (ino, n) = new_inode(S_IFREG | (perm & 0o7777), dir, false)?;
    write_inode(ino, &n)?;
    dir_add(dir, name, ino, FT_REG)?;
    sync();
    Ok((ino, true))
}

pub const S_IFSOCK: u16 = 0o140000;
const FT_SOCK: u8 = 6;

/// Cria um inode especial sem dados (por ora so sockets AF_UNIX: o bind
/// com caminho). EEXIST se o nome ja existe.
pub fn mknod_socket(at: u32, path: &[u8], perm: u16) -> Res<u32> {
    let (dir, name) = parent_of(at, path)?;
    if lookup(dir, name)?.is_some() { return Err(EEXIST); }
    let (ino, n) = new_inode(S_IFSOCK | (perm & 0o7777), dir, false)?;
    write_inode(ino, &n)?;
    dir_add(dir, name, ino, FT_SOCK)?;
    sync();
    Ok(ino)
}

pub fn mkdir(at: u32, path: &[u8], perm: u16) -> Res<u32> {
    let (dir, name) = parent_of(at, path)?;
    if lookup(dir, name)?.is_some() { return Err(EEXIST); }
    let (ino, mut n) = new_inode(S_IFDIR | (perm & 0o7777), dir, true)?;
    n.links = 2;
    let b = bmap_alloc(ino, &mut n, 0)?;
    let bs = fs().bs as usize;
    let p = bget(b, true)?;
    unsafe {
        write_dirent(p, ino, 12, b".", FT_DIR);
        write_dirent(p.add(12), dir, bs - 12, b"..", FT_DIR);
    }
    bdirty(b);
    n.set_size(bs as u64);
    write_inode(ino, &n)?;
    dir_add(dir, name, ino, FT_DIR)?;
    let mut pn = read_inode(dir)?;
    pn.links += 1;
    write_inode(dir, &pn)?;
    sync();
    Ok(ino)
}

pub fn symlink(target: &[u8], at: u32, path: &[u8]) -> Res<()> {
    if target.is_empty() || target.len() >= PATH_MAX { return Err(EINVAL); }
    let (dir, name) = parent_of(at, path)?;
    if lookup(dir, name)?.is_some() { return Err(EEXIST); }
    let (ino, mut n) = new_inode(S_IFLNK | 0o777, dir, false)?;
    if target.len() < 60 {
        unsafe { core::ptr::copy_nonoverlapping(target.as_ptr(), n.block.as_mut_ptr() as *mut u8, target.len()) };
        n.set_size(target.len() as u64);
        write_inode(ino, &n)?;
    } else {
        write_inode(ino, &n)?;
        write(ino, 0, target.as_ptr(), target.len())?;
    }
    dir_add(dir, name, ino, FT_LNK)?;
    sync();
    Ok(())
}

/// Tira um link do inode; libera-o se nao sobrou nenhum e `can_free`
/// (arquivo nao esta aberto). Retorna true se ficou sem links.
fn drop_link(ino: u32, can_free: bool) -> Res<bool> {
    let mut n = read_inode(ino)?;
    n.links = n.links.saturating_sub(1);
    n.ctime = now();
    let orphan = n.links == 0;
    if orphan && can_free {
        return release_inode(ino).map(|_| true);
    }
    write_inode(ino, &n)?;
    Ok(orphan)
}

/// Libera um inode sem links (dados e o proprio inode).
pub fn release_inode(ino: u32) -> Res<()> {
    let mut n = read_inode(ino)?;
    let dir = n.is_dir();
    if !n.fast_symlink() { truncate_inode(&mut n, 0)?; }
    n.links = 0;
    n.dtime = now();
    write_inode(ino, &n)?;
    free_inode(ino, dir)?;
    sync();
    Ok(())
}

/// unlink(2). `is_open(ino)` diz se ha descritores abertos (a liberacao
/// fica para o ultimo close). Retorna o inode, se ficou orfao e aberto.
pub fn unlink(at: u32, path: &[u8], is_open: impl Fn(u32) -> bool) -> Res<Option<u32>> {
    let (dir, name) = parent_of(at, path)?;
    let (ino, _) = lookup(dir, name)?.ok_or(ENOENT)?;
    if read_inode(ino)?.is_dir() { return Err(EISDIR); }
    dir_remove(dir, name)?;
    let open = is_open(ino);
    let orphan = drop_link(ino, !open)?;
    sync();
    Ok(if orphan && open { Some(ino) } else { None })
}

pub fn rmdir(at: u32, path: &[u8]) -> Res<()> {
    let (dir, name) = parent_of(at, path)?;
    let (ino, _) = lookup(dir, name)?.ok_or(ENOENT)?;
    if !read_inode(ino)?.is_dir() { return Err(ENOTDIR); }
    if !dir_is_empty(ino)? { return Err(ENOTEMPTY); }
    dir_remove(dir, name)?;
    release_inode(ino)?;
    let mut pn = read_inode(dir)?;
    pn.links = pn.links.saturating_sub(1);
    write_inode(dir, &pn)?;
    sync();
    Ok(())
}

/// `a` e ancestral de (ou igual a) `b`? Evita mover um diretorio para
/// dentro dele mesmo.
fn is_ancestor(a: u32, mut b: u32) -> Res<bool> {
    for _ in 0..256 {
        if a == b { return Ok(true); }
        if b == ROOT_INO { return Ok(false); }
        b = lookup(b, b"..")?.map(|(i, _)| i).ok_or(EIO)?;
    }
    Ok(false)
}

pub fn rename(fat: u32, from: &[u8], tat: u32, to: &[u8], is_open: impl Fn(u32) -> bool) -> Res<Option<u32>> {
    let (sdir, sname) = parent_of(fat, from)?;
    let (ddir, dname) = parent_of(tat, to)?;
    let (ino, ft) = lookup(sdir, sname)?.ok_or(ENOENT)?;
    let moving_dir = read_inode(ino)?.is_dir();
    if moving_dir && sdir != ddir && is_ancestor(ino, ddir)? { return Err(EINVAL); }
    let mut orphan = None;
    if let Some((old, _)) = lookup(ddir, dname)? {
        if old == ino { return Ok(None); }
        let on = read_inode(old)?;
        if on.is_dir() {
            if !moving_dir { return Err(EISDIR); }
            if !dir_is_empty(old)? { return Err(ENOTEMPTY); }
            dir_remove(ddir, dname)?;
            release_inode(old)?;
            let mut pn = read_inode(ddir)?;
            pn.links = pn.links.saturating_sub(1);
            write_inode(ddir, &pn)?;
        } else {
            if moving_dir { return Err(ENOTDIR); }
            dir_remove(ddir, dname)?;
            let open = is_open(old);
            if drop_link(old, !open)? && open { orphan = Some(old); }
        }
    }
    dir_add(ddir, dname, ino, ft)?;
    dir_remove(sdir, sname)?;
    if moving_dir && sdir != ddir {
        set_dotdot(ino, ddir)?;
        let mut a = read_inode(sdir)?;
        a.links = a.links.saturating_sub(1);
        write_inode(sdir, &a)?;
        let mut b = read_inode(ddir)?;
        b.links += 1;
        write_inode(ddir, &b)?;
    }
    sync();
    Ok(orphan)
}

/// Caminho absoluto do diretorio `dir`, subindo pelos "..". Retorna o
/// tamanho escrito em `out` (sem NUL).
pub fn dir_path(dir: u32, out: &mut [u8]) -> Res<usize> {
    let mut tmp = [0u8; PATH_MAX];
    let mut at = PATH_MAX;      // monta de tras para frente em tmp
    let mut cur = dir;
    for _ in 0..256 {
        if cur == ROOT_INO { break; }
        let (parent, _) = lookup(cur, b"..")?.ok_or(ENOENT)?;
        // acha o nome de `cur` dentro do pai
        let mut pos = 0;
        let mut found = false;
        while let Some(d) = readdir(parent, pos)? {
            pos = d.next;
            if d.ino == cur && d.name() != b"." && d.name() != b".." {
                if at < d.name_len + 1 { return Err(ENAMETOOLONG); }
                at -= d.name_len;
                tmp[at..at + d.name_len].copy_from_slice(d.name());
                at -= 1;
                tmp[at] = b'/';
                found = true;
                break;
            }
        }
        if !found { return Err(ENOENT); }
        cur = parent;
    }
    if at == PATH_MAX { at -= 1; tmp[at] = b'/'; }
    let n = PATH_MAX - at;
    if n > out.len() { return Err(ENAMETOOLONG); }
    out[..n].copy_from_slice(&tmp[at..]);
    Ok(n)
}
