//! Bolhas — demo de fisica do Pulsar OS: bolas com gravidade, colisoes
//! elasticas e sombreamento, desenhadas na propria superficie. A fisica
//! roda em passo fixo de 60 Hz (independente da taxa de quadros) e o app
//! roda em paralelo com o compositor, em outro nucleo.
//! Clique cria bolas, Espaco pausa, R reinicia, + adiciona.
#![no_std]
#![no_main]
use plib::*;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { exit() }

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! { run(); exit() }

const MAXB: usize = 24;
const FX: i32 = 256; // ponto fixo 8 bits
const FLOOR: i32 = 14; // margem inferior (deixa a sombra visivel)

#[derive(Clone, Copy)]
struct Ball { x: i32, y: i32, vx: i32, vy: i32, r: i32, color: u32 }

const COLORS: [u32; 8] = [0xFF453A, 0xFF9F0A, 0xFFD60A, 0x30D158, 0x64D2FF, 0x0A84FF, 0xBF5AF2, 0xFF375F];

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 17; self.0 ^= self.0 << 5; self.0 }
    fn range(&mut self, a: i32, b: i32) -> i32 { a + (self.next() % (b - a).max(1) as u32) as i32 }
}

fn spawn_ball(balls: &mut [Ball; MAXB], n: &mut usize, rng: &mut Rng, x: i32, y: i32) {
    if *n >= MAXB { return; }
    let r = rng.range(14, 34);
    balls[*n] = Ball {
        x: x * FX, y: y * FX,
        vx: rng.range(-5 * FX, 5 * FX), vy: rng.range(-6 * FX, -FX),
        r, color: COLORS[(rng.next() % 8) as usize],
    };
    *n += 1;
}

fn physics(balls: &mut [Ball], w: i32, h: i32) {
    for b in balls.iter_mut() {
        b.vy += FX * 35 / 100; // gravidade
        b.x += b.vx;
        b.y += b.vy;
        let r = b.r * FX;
        if b.x < r { b.x = r; b.vx = -b.vx * 92 / 100; }
        if b.x > w - r { b.x = w - r; b.vx = -b.vx * 92 / 100; }
        if b.y > h - r {
            b.y = h - r;
            b.vy = -b.vy * 86 / 100;
            b.vx = b.vx * 98 / 100;
            if b.vy.abs() < FX { b.vy = 0; }
        }
        if b.y < r { b.y = r; b.vy = -b.vy; }
    }
    // colisoes entre bolas (massa ~ r^2)
    let n = balls.len();
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (balls[i], balls[j]);
            let (dx, dy) = ((b.x - a.x) / 16, (b.y - a.y) / 16);
            let rr = (a.r + b.r) * FX / 16;
            let d2 = dx * dx + dy * dy;
            if d2 >= rr * rr || d2 == 0 { continue; }
            let d = gfx::isqrt(d2 as u32) as i32;
            let (nx, ny) = (dx * 1024 / d.max(1), dy * 1024 / d.max(1));
            let rel = ((b.vx - a.vx) as i64 * nx as i64 + (b.vy - a.vy) as i64 * ny as i64) / 1024;
            let (ma, mb) = ((a.r * a.r) as i64, (b.r * b.r) as i64);
            if rel < 0 {
                let imp = -2 * rel * 95 / 100;
                let ja = (imp * mb / (ma + mb)) as i32;
                let jb = (imp * ma / (ma + mb)) as i32;
                balls[i].vx -= ja * nx / 1024; balls[i].vy -= ja * ny / 1024;
                balls[j].vx += jb * nx / 1024; balls[j].vy += jb * ny / 1024;
            }
            // separa as bolas sobrepostas
            let push = (rr - d) * 16 / 2;
            balls[i].x -= push * nx / 1024; balls[i].y -= push * ny / 1024;
            balls[j].x += push * nx / 1024; balls[j].y += push * ny / 1024;
        }
    }
}

fn draw(app: &App, balls: &[Ball], fps: u32, paused: bool) {
    let fb = app.fb;
    let th = app.theme;
    let (top, bot) = if th.dark { (0x0D1020, 0x1B2340) } else { (0xE8F0FF, 0xC9D8F5) };
    fb.vgrad(Rect::new(0, 0, app.w, app.h), top, bot);
    let floor = (app.h - FLOOR) * 16;
    for b in balls.iter() {
        let (cx, cy, r) = (b.x / 16, b.y / 16, b.r * 16);
        // sombra no chao: mais fraca quanto mais alta a bola
        let dist = (floor - cy - r).max(0);
        let sa = (90 - dist / 32).clamp(10, 90) as u32;
        let spread = r * 8 / 10 + dist / 24;
        fb.ellipse16(cx, floor + 4, spread, (spread / 5).max(24), 0x000000, sa);
        fb.disc16(cx, cy, r, gfx::shade(b.color, -60), 255);
        fb.disc16(cx - r / 10, cy - r / 10, r * 86 / 100, b.color, 255);
        fb.disc16(cx - r / 4, cy - r / 4, r * 45 / 100, gfx::shade(b.color, 90), 140);
        fb.disc16(cx - r * 4 / 10, cy - r * 4 / 10, r / 6, 0xFFFFFF, 200);
    }
    let mut s = Buf::<48>::new();
    s.u(balls.len() as u64).s(" bolhas · ").u(fps as u64).s(" qps");
    if paused { s.s(" · pausado"); }
    let pill = Rect::new(10, 10, FONT_SMALL.width(s.get()) + 20, 24);
    fb.round_rect_a(pill, 12, 0x000000, 90);
    fb.text(&FONT_SMALL, 20, 14, s.get(), 0xFFFFFF);
}

fn run() {
    let mut app = App::new(2, "Bolhas".as_bytes(), 480, 360);
    let mut rng = Rng(uptime_ms() as u32 | 1);
    let mut balls = [Ball { x: 0, y: 0, vx: 0, vy: 0, r: 0, color: 0 }; MAXB];
    let mut n = 0;
    for k in 0..7 { spawn_ball(&mut balls, &mut n, &mut rng, 60 + k * 50, 80 + (k % 3) * 40); }
    let mut paused = false;
    let (mut frames, mut fps) = (0u32, 0u32);
    let mut t_fps = uptime_ms();
    let mut last_step = uptime_ms();
    let mut ev = app.commit();
    loop {
        if ev.close { exit(); }
        if ev.click { spawn_ball(&mut balls, &mut n, &mut rng, ev.mx, ev.my); }
        let mut act = ev.menu;
        if let Some(k) = ev.key {
            act = match k.ch {
                32 => APP_MENU_PAUSE,
                0x72 | 0x52 => APP_MENU_RESET,
                0x2B | 0x3D => APP_MENU_ADD,
                _ => 0,
            };
        }
        match act {
            APP_MENU_PAUSE => paused = !paused,
            APP_MENU_RESET => {
                n = 0;
                for k in 0..7 { spawn_ball(&mut balls, &mut n, &mut rng, 60 + k * 50, 80); }
            }
            APP_MENU_ADD => { let w = app.w; spawn_ball(&mut balls, &mut n, &mut rng, w / 2, 60); }
            _ => {}
        }
        if ev.more { ev = app.poll(); continue; }
        // passo fixo de 60 Hz (no maximo 4 passos por quadro)
        let now = uptime_ms();
        if paused || now - last_step > 100 { last_step = now; }
        let steps = (now - last_step) / 16;
        last_step += steps * 16;
        let (w, h) = (app.w * FX, (app.h - FLOOR) * FX);
        for _ in 0..steps.min(4) { physics(&mut balls[..n], w, h); }
        draw(&app, &balls[..n], fps, paused);
        frames += 1;
        if now - t_fps >= 1000 { fps = frames; frames = 0; t_fps = now; }
        ev = app.commit();
    }
}
