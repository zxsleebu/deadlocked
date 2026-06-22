use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicI8, Ordering};

use glam::{Vec2, Vec3};

use crate::cs2::{CS2, bones::Bones, entity::player::Player};

const TWO_PI: f32 = 2.0 * PI;
const NEEDED_TICKS: i32 = 2;

static UNAVAILABLE_WARNED: AtomicBool = AtomicBool::new(false);
static VDATA_WARNED: AtomicBool = AtomicBool::new(false);
static READY_INFOED: AtomicBool = AtomicBool::new(false);
static LAST_VERDICT: AtomicI8 = AtomicI8::new(-1);

macro_rules! hb_fail {
    ($flag:expr, $($args:tt)+) => {
        if !$flag.swap(true, Ordering::Relaxed) {
            utils::warn!($($args)+);
        }
    };
}

static HB_WARN_SCENE: AtomicBool = AtomicBool::new(false);
static HB_WARN_BONE_ARRAY: AtomicBool = AtomicBool::new(false);
static HB_WARN_MODEL_HANDLE: AtomicBool = AtomicBool::new(false);
static HB_WARN_CMODEL: AtomicBool = AtomicBool::new(false);
static HB_WARN_RENDER_MESHES: AtomicBool = AtomicBool::new(false);
static HB_WARN_HITBOX_DATA: AtomicBool = AtomicBool::new(false);
static HB_WARN_COUNT: AtomicBool = AtomicBool::new(false);
static HB_WARN_ARRAY_PTR: AtomicBool = AtomicBool::new(false);
static HB_WARN_EMPTY: AtomicBool = AtomicBool::new(false);
static HB_OK_INFOED: AtomicBool = AtomicBool::new(false);
static HB_SCAN_DONE: AtomicBool = AtomicBool::new(false);

pub const SEED_SYNC_CAPSULES: &[(Bones, Bones, f32)] = &[
    (Bones::Head, Bones::Head, 3.5),
    (Bones::Neck, Bones::Spine4, 3.5),
    (Bones::Spine4, Bones::Spine3, 7.0),
    (Bones::Spine3, Bones::Spine2, 7.0),
    (Bones::Spine2, Bones::Spine1, 7.0),
    (Bones::Spine1, Bones::Hip, 7.0),
    (Bones::LeftShoulder, Bones::LeftElbow, 3.5),
    (Bones::LeftElbow, Bones::LeftHand, 3.0),
    (Bones::RightShoulder, Bones::RightElbow, 3.5),
    (Bones::RightElbow, Bones::RightHand, 3.0),
    (Bones::LeftHip, Bones::LeftKnee, 4.5),
    (Bones::LeftKnee, Bones::LeftFoot, 3.5),
    (Bones::RightHip, Bones::RightKnee, 4.5),
    (Bones::RightKnee, Bones::RightFoot, 3.5),
];

const HITBOX_BONE_MAP: [i32; 19] = [
    7, 6, 1, 2, 3, 4, 5, 17, 20, 18, 21, 19, 22, 11, 15, 10, 9, 14, 13,
];

fn rotate_by_quat(q: [f32; 4], v: Vec3) -> Vec3 {
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * v.z - qz * v.y);
    let ty = 2.0 * (qz * v.x - qx * v.z);
    let tz = 2.0 * (qx * v.y - qy * v.x);
    Vec3::new(
        v.x + qw * tx + (qy * tz - qz * ty),
        v.y + qw * ty + (qz * tx - qx * tz),
        v.z + qw * tz + (qx * ty - qy * tx),
    )
}

struct ValveRng {
    state: i32,
    index: i32,
    table: [i32; 32],
    seeded: bool,
}

impl ValveRng {
    fn new() -> Self {
        Self {
            state: 0,
            index: 0,
            table: [0; 32],
            seeded: false,
        }
    }

    fn seed(&mut self, s: i32) {
        self.state = s.wrapping_abs().wrapping_neg();
        self.index = 0;
        self.seeded = false;
    }

    fn generate(&mut self) -> i32 {
        if !self.seeded {
            let mut v = -self.state;
            if v < 1 {
                v = 1;
            }

            for j in (0..=39i32).rev() {
                v = Self::lcg(v);
                if j < 32 {
                    self.table[j as usize] = v;
                }
            }

            self.state = v;
            self.index = self.table[0];
            self.seeded = true;
        }

        self.state = Self::lcg(self.state);
        let idx = (self.index / 0x4000000) as usize;
        self.index = self.table[idx];
        self.table[idx] = self.state;
        self.index
    }

    #[allow(clippy::excessive_precision)]
    fn random_float(&mut self, min: f32, max: f32) -> f32 {
        let raw = self.generate() as f32;
        let norm = 0.99999988f32.min(raw * 4.6566129e-10);
        min + norm * (max - min)
    }

    fn lcg(state: i32) -> i32 {
        let k = state / 127773;
        let mut result = 16807i32
            .wrapping_mul(state - k * 127773)
            .wrapping_sub(2836i32.wrapping_mul(k));
        if result < 0 {
            result += 2147483647;
        }
        result
    }
}

struct Sha1 {
    state: [u32; 5],
    count: u64,
    buffer: [u8; 64],
    digest: [u8; 20],
}

impl Sha1 {
    fn new() -> Self {
        let mut s = Self {
            state: [0; 5],
            count: 0,
            buffer: [0; 64],
            digest: [0; 20],
        };
        s.reset();
        s
    }

    fn reset(&mut self) {
        self.state[0] = 0x67452301;
        self.state[1] = 0xEFCDAB89;
        self.state[2] = 0x98BADCFE;
        self.state[3] = 0x10325476;
        self.state[4] = 0xC3D2E1F0;
        self.count = 0;
    }

    fn update(&mut self, data: &[u8]) {
        let len = data.len();
        let index = (self.count & 63) as usize;
        self.count = self.count.wrapping_add(len as u64);

        let mut i = 0usize;

        if index != 0 {
            let part_len = 64 - index;
            if len >= part_len {
                self.buffer[index..index + part_len].copy_from_slice(&data[..part_len]);
                let block = self.buffer;
                self.transform(&block);
                i = part_len;
            } else {
                self.buffer[index..index + len].copy_from_slice(&data[..len]);
                return;
            }
        }

        while i + 64 <= len {
            self.transform(&data[i..]);
            i += 64;
        }

        if i < len {
            let remaining = len - i;
            self.buffer[..remaining].copy_from_slice(&data[i..i + remaining]);
        }
    }

    fn finalize(&mut self) {
        let mut padding = [0u8; 64];
        padding[0] = 0x80;

        let index = (self.count & 63) as usize;
        let pad_len = if index < 56 { 56 - index } else { 120 - index };
        let bit_count = self.count.wrapping_mul(8);

        self.update(&padding[..pad_len]);

        let mut bits = [0u8; 8];
        for i in 0..8 {
            bits[7 - i] = (bit_count >> (i * 8)) as u8;
        }
        self.update(&bits);

        for i in 0..5 {
            self.digest[i * 4] = (self.state[i] >> 24) as u8;
            self.digest[i * 4 + 1] = (self.state[i] >> 16) as u8;
            self.digest[i * 4 + 2] = (self.state[i] >> 8) as u8;
            self.digest[i * 4 + 3] = self.state[i] as u8;
        }
    }

    fn first_uint32(&self) -> u32 {
        u32::from_le_bytes([
            self.digest[0],
            self.digest[1],
            self.digest[2],
            self.digest[3],
        ])
    }

    fn rotl(v: u32, n: u32) -> u32 {
        v.rotate_left(n)
    }

    #[allow(clippy::needless_range_loop)]
    fn transform(&mut self, block: &[u8]) {
        debug_assert!(block.len() >= 64);

        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = (block[i * 4] as u32) << 24
                | (block[i * 4 + 1] as u32) << 16
                | (block[i * 4 + 2] as u32) << 8
                | (block[i * 4 + 3] as u32);
        }
        for i in 16..80 {
            w[i] = Self::rotl(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for i in 0..80 {
            let (f, k) = if i < 20 {
                ((b & c) | (!b & d), 0x5A827999u32)
            } else if i < 40 {
                (b ^ c ^ d, 0x6ED9EBA1u32)
            } else if i < 60 {
                ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32)
            } else {
                (b ^ c ^ d, 0xCA62C1D6u32)
            };

            let temp = Self::rotl(a, 5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = Self::rotl(b, 30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

#[allow(clippy::excessive_precision)]
fn normalize_angle(a: f32) -> f32 {
    a - (a * 0.0027777778 + 0.5).floor() * 360.0
}

fn quantize_angle(a: f32) -> f32 {
    (normalize_angle(a) * 2.0).floor() * 0.5
}

fn spread_seed(pitch: f32, yaw: f32, tick: i32) -> u32 {
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&quantize_angle(pitch).to_le_bytes());
    buf[4..8].copy_from_slice(&quantize_angle(yaw).to_le_bytes());
    buf[8..12].copy_from_slice(&tick.to_le_bytes());

    let mut hash = Sha1::new();
    hash.update(&buf);
    hash.finalize();
    hash.first_uint32()
}

fn calculate_spread(
    seed: i32,
    inaccuracy: f32,
    spread: f32,
    recoil_index: f32,
    item_def_idx: u16,
    num_bullets: i32,
) -> Vec2 {
    const REVOLVER_ID: u16 = 64;
    const NEGEV_ID: u16 = 28;

    let mut rng = ValveRng::new();
    rng.seed(seed);

    let mut inac_r = rng.random_float(0.0, 1.0);
    let inac_a = rng.random_float(0.0, TWO_PI);

    if item_def_idx == REVOLVER_ID && num_bullets == 1 {
        inac_r = 1.0 - (inac_r * inac_r);
    } else if item_def_idx == NEGEV_ID && recoil_index < 3.0 {
        let mut v = inac_r;
        let mut c = 3i32;
        loop {
            c -= 1;
            v *= v;
            if (c as f32) <= recoil_index {
                break;
            }
        }
        inac_r = 1.0 - v;
    }

    inac_r *= inaccuracy;

    let mut spr_r = rng.random_float(0.0, 1.0);
    let spr_a = rng.random_float(0.0, TWO_PI);

    if item_def_idx == REVOLVER_ID && num_bullets == 1 {
        spr_r = 1.0 - (spr_r * spr_r);
    } else if item_def_idx == NEGEV_ID && recoil_index < 3.0 {
        let mut v = spr_r;
        let mut c = 3i32;
        loop {
            c -= 1;
            v *= v;
            if (c as f32) <= recoil_index {
                break;
            }
        }
        spr_r = 1.0 - v;
    }

    spr_r *= spread;

    Vec2::new(
        spr_a.cos() * spr_r + inac_a.cos() * inac_r,
        spr_a.sin() * spr_r + inac_a.sin() * inac_r,
    )
}

fn ray_hits_capsule(ray_origin: Vec3, ray_dir: Vec3, start: Vec3, end: Vec3, radius: f32) -> bool {
    let capsule_vec = end - start;
    let capsule_length = capsule_vec.length();

    if capsule_length < 0.001 {
        let to_center = start - ray_origin;
        let projection = to_center.dot(ray_dir);
        if projection < 0.0 {
            return false;
        }
        let closest = ray_origin + ray_dir * projection;
        return (closest - start).dot(closest - start) <= radius * radius;
    }

    let capsule_dir = capsule_vec / capsule_length;
    let w = ray_origin - start;

    let a = ray_dir.dot(ray_dir);
    let b = ray_dir.dot(capsule_dir);
    let c = capsule_dir.dot(capsule_dir);
    let d = ray_dir.dot(w);
    let e = capsule_dir.dot(w);

    let denom = a * c - b * b;

    let (s, mut t) = if denom.abs() < 0.0001 {
        (0.0, if b > c { d / b } else { e / c })
    } else {
        ((b * e - c * d) / denom, (a * e - b * d) / denom)
    };

    t = t.clamp(0.0, capsule_length);
    if s < 0.0 {
        return false;
    }

    let point_on_capsule = start + capsule_dir * t;
    let point_on_ray = ray_origin + ray_dir * s;

    (point_on_ray - point_on_capsule).dot(point_on_ray - point_on_capsule) <= radius * radius
}

fn angle_vectors(view: Vec2) -> (Vec3, Vec3, Vec3) {
    let deg2rad = PI / 180.0;
    let sp = (view.x * deg2rad).sin();
    let cp = (view.x * deg2rad).cos();
    let sy = (view.y * deg2rad).sin();
    let cy = (view.y * deg2rad).cos();

    let forward = Vec3::new(cp * cy, cp * sy, -sp);
    let right = Vec3::new(-sy, cy, 0.0);
    let up = Vec3::new(sp * cy, sp * sy, cp);
    (forward, right, up)
}

impl CS2 {
    fn missing_required_offsets(&self) -> Vec<&'static str> {
        let so = &self.offsets.seed_sync;
        let required: [(Option<u64>, &str); 20] = [
            (
                so.weapon_vdata_ptr,
                "C_BaseEntity::m_nSubclassID (+8 -> vdata)",
            ),
            (so.spread, "CCSWeaponBaseVData::m_flSpread"),
            (
                so.inaccuracy_crouch,
                "CCSWeaponBaseVData::m_flInaccuracyCrouch",
            ),
            (
                so.inaccuracy_stand,
                "CCSWeaponBaseVData::m_flInaccuracyStand",
            ),
            (
                so.inaccuracy_ladder,
                "CCSWeaponBaseVData::m_flInaccuracyLadder",
            ),
            (so.inaccuracy_move, "CCSWeaponBaseVData::m_flInaccuracyMove"),
            (so.max_speed, "CCSWeaponBaseVData::m_flMaxSpeed"),
            (
                so.inaccuracy_jump_initial,
                "CCSWeaponBaseVData::m_flInaccuracyJumpInitial",
            ),
            (
                so.inaccuracy_jump_apex,
                "CCSWeaponBaseVData::m_flInaccuracyJumpApex",
            ),
            (
                so.recovery_time_crouch,
                "CCSWeaponBaseVData::m_flRecoveryTimeCrouch",
            ),
            (
                so.recovery_time_stand,
                "CCSWeaponBaseVData::m_flRecoveryTimeStand",
            ),
            (
                so.recovery_time_crouch_final,
                "CCSWeaponBaseVData::m_flRecoveryTimeCrouchFinal",
            ),
            (
                so.recovery_time_stand_final,
                "CCSWeaponBaseVData::m_flRecoveryTimeStandFinal",
            ),
            (
                so.recovery_transition_start,
                "CCSWeaponBaseVData::m_nRecoveryTransitionStartBullet",
            ),
            (
                so.recovery_transition_end,
                "CCSWeaponBaseVData::m_nRecoveryTransitionEndBullet",
            ),
            (so.is_revolver, "CCSWeaponBaseVData::m_bIsRevolver"),
            (so.weapon_mode, "C_CSWeaponBase::m_weaponMode"),
            (
                so.turning_inaccuracy,
                "C_CSWeaponBase::m_flTurningInaccuracy",
            ),
            (so.accuracy_penalty, "C_CSWeaponBase::m_fAccuracyPenalty"),
            (so.recoil_index, "C_CSWeaponBase::m_iRecoilIndex"),
        ];

        required
            .into_iter()
            .filter(|(o, _)| o.is_none())
            .map(|(_, n)| n)
            .collect()
    }

    fn active_weapon_entity(&self, local: &Player) -> Option<u64> {
        let weapon_services: u64 = self
            .process
            .read(local.pawn + self.offsets.pawn.weapon_services);
        if weapon_services == 0 {
            return None;
        }
        let handle: i32 = self
            .process
            .read(weapon_services + self.offsets.weapon_services.active_weapon);
        let index = handle as u64 & 0xFFF;
        Player::get_client_entity(self, index)
    }

    fn firing_mode_float(&self, vdata: u64, off: Option<u64>, fire_mode: i32) -> f32 {
        let Some(off) = off else {
            return 0.0;
        };
        let pair: [f32; 2] = self.process.read(vdata + off);
        if fire_mode != 0 { pair[1] } else { pair[0] }
    }

    fn convar_float(&self, addr: Option<u64>, default: f32) -> f32 {
        addr.map(|a| self.process.read::<f32>(a + 0x58))
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    }

    fn convar_bool(&self, addr: Option<u64>, default: bool) -> bool {
        addr.map(|a| self.process.read::<u8>(a + 0x58) != 0)
            .unwrap_or(default)
    }

    fn tick_count(&self) -> i32 {
        let global_vars: u64 = self.process.read(self.offsets.direct.global_vars);
        self.process.read(global_vars + 0x44)
    }

    fn compute_inaccuracy(&self, local_pawn: u64, weapon: u64, vdata: u64) -> f32 {
        let so = &self.offsets.seed_sync;

        let fire_mode: i32 = self.process.read(weapon + so.weapon_mode.unwrap_or(0));
        let turning_inaccuracy: f32 = self
            .process
            .read(weapon + so.turning_inaccuracy.unwrap_or(0));
        let accuracy_penalty: f32 = self.process.read(weapon + so.accuracy_penalty.unwrap_or(0));
        let recoil_index: i32 = self.process.read(weapon + so.recoil_index.unwrap_or(0));

        let fm = |pair: [f32; 2]| -> f32 { if fire_mode != 0 { pair[1] } else { pair[0] } };

        let inaccuracy_crouch: [f32; 2] =
            self.process.read(vdata + so.inaccuracy_crouch.unwrap_or(0));
        let inaccuracy_stand: [f32; 2] =
            self.process.read(vdata + so.inaccuracy_stand.unwrap_or(0));
        let inaccuracy_ladder: [f32; 2] =
            self.process.read(vdata + so.inaccuracy_ladder.unwrap_or(0));
        let inaccuracy_move: [f32; 2] = self.process.read(vdata + so.inaccuracy_move.unwrap_or(0));
        let max_speed: [f32; 2] = self.process.read(vdata + so.max_speed.unwrap_or(0));
        let inaccuracy_jump_initial: f32 = self
            .process
            .read(vdata + so.inaccuracy_jump_initial.unwrap_or(0));
        let inaccuracy_jump_apex: f32 = self
            .process
            .read(vdata + so.inaccuracy_jump_apex.unwrap_or(0));
        let recovery_time_crouch: f32 = self
            .process
            .read(vdata + so.recovery_time_crouch.unwrap_or(0));
        let recovery_time_stand: f32 = self
            .process
            .read(vdata + so.recovery_time_stand.unwrap_or(0));
        let recovery_time_crouch_final: f32 = self
            .process
            .read(vdata + so.recovery_time_crouch_final.unwrap_or(0));
        let recovery_time_stand_final: f32 = self
            .process
            .read(vdata + so.recovery_time_stand_final.unwrap_or(0));
        let recovery_transition_start: i32 = self
            .process
            .read(vdata + so.recovery_transition_start.unwrap_or(0));
        let recovery_transition_end: i32 = self
            .process
            .read(vdata + so.recovery_transition_end.unwrap_or(0));
        let is_revolver: bool = self.process.read::<u8>(vdata + so.is_revolver.unwrap_or(0)) != 0;

        let flags: u32 = self.process.read(local_pawn + self.offsets.pawn.flags);
        let velocity: Vec3 = self.process.read(local_pawn + self.offsets.pawn.velocity);
        let speed = Vec2::new(velocity.x, velocity.y).length();
        let move_type: u8 = so
            .move_type
            .map(|o| self.process.read(local_pawn + o))
            .unwrap_or(0);
        let is_walking: bool = so
            .is_walking
            .map(|o| self.process.read::<u8>(local_pawn + o) != 0)
            .unwrap_or(false);
        let on_ground = (flags & 1) != 0;
        let crouching = (flags & 2) != 0;

        let forcespread = self.convar_float(so.convar_forcespread, 0.0);
        if forcespread > 0.0 {
            return forcespread.min(1.0);
        }

        let nospread = self.convar_bool(so.convar_nospread, false);
        if nospread {
            return 0.0;
        }

        let base_inaccuracy = if move_type == 9 {
            fm(inaccuracy_stand) + fm(inaccuracy_ladder)
        } else if crouching {
            fm(inaccuracy_crouch)
        } else {
            fm(inaccuracy_stand)
        };

        let _ = (
            base_inaccuracy,
            recovery_time_crouch,
            recovery_time_stand,
            recovery_time_crouch_final,
            recovery_time_stand_final,
            recovery_transition_start,
            recovery_transition_end,
            recoil_index,
        );

        let max_spd = fm(max_speed);
        let edge0 = max_spd * 0.34;
        let edge1 = max_spd * 0.95;

        let mut move_factor = if edge0 == edge1 {
            if speed - edge1 >= 0.0 { 1.0 } else { 0.0 }
        } else {
            ((speed - edge0) / (edge1 - edge0)).clamp(0.0, 1.0)
        };

        let mut move_inaccuracy = 0.0;
        if move_factor > 0.0 {
            if !is_revolver {
                move_factor = move_factor.powf(0.25);
            }
            move_inaccuracy = move_factor * fm(inaccuracy_move);
        }

        let mut total = turning_inaccuracy + move_inaccuracy;

        if move_type != 9 && !on_ground {
            let impulse = self.convar_float(so.convar_jump_impulse, 301.993);
            let jump_vel = impulse.abs().sqrt();
            let cur_vel = velocity.z.abs().sqrt();
            let lo = jump_vel * 0.25;

            let air = if lo == jump_vel {
                if cur_vel - jump_vel >= 0.0 {
                    inaccuracy_jump_initial
                } else {
                    inaccuracy_jump_apex
                }
            } else {
                let frac = (cur_vel - lo) / (jump_vel - lo);
                inaccuracy_jump_apex + frac * (inaccuracy_jump_initial - inaccuracy_jump_apex)
            };

            let air = if air < 0.0 {
                0.0
            } else {
                (inaccuracy_jump_initial * 2.0).min(air)
            };

            total += air;
        }

        total += accuracy_penalty;

        let _ = is_walking;
        let shotgun_patterns = self.convar_bool(so.convar_shotgun_patterns, false);
        let _ = shotgun_patterns;

        1.0_f32.min(total)
    }

    fn scan_scene_node_for_model(&self, scene_node: u64) {
        if HB_SCAN_DONE.swap(true, Ordering::Relaxed) {
            return;
        }
        utils::warn!(
            "seed-sync hitbox: scanning scene_node={:#x} for model handle (0x100..0x300, step 8)",
            scene_node
        );
        let mut validated: Option<(u64, u64, i32)> = None;
        for off in (0x100..0x300).step_by(8) {
            let val: u64 = self.process.read(scene_node + off);
            if val == 0 {
                continue;
            }
            // pointer-like: within plausible heap range
            if !(0x10000..=0x7fffffffffff).contains(&val) {
                continue;
            }
            let cmodel: u64 = self.process.read(val);
            if cmodel == 0 {
                continue;
            }
            let render_meshes: u64 = self
                .process
                .read::<u64>(self.process.read::<u64>(cmodel + 0x78));
            if render_meshes == 0 {
                continue;
            }
            let hitbox_data: u64 = self.process.read(render_meshes + 0x150);
            if hitbox_data == 0 {
                continue;
            }
            let count: i32 = self.process.read(hitbox_data + 0x28);
            if count <= 0 || count > 20 {
                continue;
            }
            utils::warn!(
                "seed-sync hitbox: SCAN MATCH offset=+{:#x} val={:#x} cmodel={:#x} count={}",
                off,
                val,
                cmodel,
                count
            );
            if validated.is_none() {
                validated = Some((off, val, count));
            }
        }
        match validated {
            Some((off, val, count)) => {
                utils::warn!(
                    "seed-sync hitbox: SCAN RESULT -> model handle at scene_node+{:#x} (val={:#x}, count={}); set game_scene_node.model = {:#x}",
                    off,
                    val,
                    count,
                    off
                );
            }
            None => {
                utils::warn!(
                    "seed-sync hitbox: SCAN RESULT -> no offset in 0x100..0x300 validated the cmodel chain; model handle may be a CStrongHandle requiring handle-table translation, or lives outside this range"
                );
            }
        }
    }

    fn real_hitbox_capsules(&self, target: &Player) -> Option<Vec<(Vec3, Vec3, f32)>> {
        let scene_node: u64 = self
            .process
            .read(target.pawn + self.offsets.pawn.game_scene_node);
        if scene_node == 0 {
            hb_fail!(
                HB_WARN_SCENE,
                "seed-sync hitbox: scene_node null (pawn={:#x})",
                target.pawn
            );
            return None;
        }

        let bone_array: u64 = self.process.read(
            scene_node
                + self.offsets.game_scene_node.model_state
                + self.offsets.model_state.skeleton_instance,
        );
        if bone_array == 0 {
            hb_fail!(
                HB_WARN_BONE_ARRAY,
                "seed-sync hitbox: bone_array null (scene_node={:#x})",
                scene_node
            );
            return None;
        }

        let model_offset = if self.offsets.game_scene_node.model != 0 {
            self.offsets.game_scene_node.model
        } else {
            0x160
        };
        let model_handle: u64 = self.process.read(scene_node + model_offset);
        if model_handle == 0 {
            hb_fail!(
                HB_WARN_MODEL_HANDLE,
                "seed-sync hitbox: model_handle null (scene_node={:#x} +{:#x})",
                scene_node,
                model_offset
            );
            self.scan_scene_node_for_model(scene_node);
            return None;
        }
        let cmodel: u64 = self.process.read(model_handle);
        if cmodel == 0 {
            hb_fail!(
                HB_WARN_CMODEL,
                "seed-sync hitbox: cmodel null (model_handle={:#x})",
                model_handle
            );
            return None;
        }
        let render_meshes: u64 = self.process.read(self.process.read::<u64>(cmodel + 0x78));
        if render_meshes == 0 {
            hb_fail!(
                HB_WARN_RENDER_MESHES,
                "seed-sync hitbox: render_meshes null (cmodel={:#x}, deref cmodel+0x78 twice)",
                cmodel
            );
            return None;
        }
        let hitbox_data: u64 = self.process.read(render_meshes + 0x150);
        if hitbox_data == 0 {
            hb_fail!(
                HB_WARN_HITBOX_DATA,
                "seed-sync hitbox: hitbox_data null (render_meshes={:#x} +0x150)",
                render_meshes
            );
            return None;
        }
        let count: i32 = self.process.read(hitbox_data + 0x28);
        if count <= 0 || count > 20 {
            hb_fail!(
                HB_WARN_COUNT,
                "seed-sync hitbox: count invalid (count={}, hitbox_data={:#x} +0x28)",
                count,
                hitbox_data
            );
            return None;
        }
        let array_ptr: u64 = self.process.read(hitbox_data + 0x30);
        if array_ptr == 0 {
            hb_fail!(
                HB_WARN_ARRAY_PTR,
                "seed-sync hitbox: array_ptr null (hitbox_data={:#x} +0x30)",
                hitbox_data
            );
            return None;
        }

        let origin: Vec3 = self
            .process
            .read(scene_node + self.offsets.game_scene_node.origin);

        let mut capsules = Vec::with_capacity(count as usize);
        for i in 0..count {
            if i as usize >= HITBOX_BONE_MAP.len() {
                break;
            }
            let bone = HITBOX_BONE_MAP[i as usize];
            if bone < 0 {
                continue;
            }

            let hb_base = array_ptr + (i as u64) * 0x70;
            let mins: Vec3 = self.process.read(hb_base + 0x18);
            let maxs: Vec3 = self.process.read(hb_base + 0x24);
            let radius: f32 = self.process.read(hb_base + 0x30);
            if !(0.0..=100.0).contains(&radius) {
                continue;
            }

            let bone_base = bone_array + (bone as u64) * 32;
            let bone_pos: Vec3 = self.process.read(bone_base);
            if bone_pos.distance(origin) > 256.0 {
                continue;
            }
            let bone_rot: [f32; 4] = self.process.read(bone_base + 0x10);

            let start_world = bone_pos + rotate_by_quat(bone_rot, mins);
            let end_world = bone_pos + rotate_by_quat(bone_rot, maxs);

            capsules.push((start_world, end_world, radius));
        }

        if capsules.is_empty() {
            hb_fail!(
                HB_WARN_EMPTY,
                "seed-sync hitbox: parsed 0 valid capsules (count={count}, array_ptr={array_ptr:#x})",
            );
            return None;
        }

        if !HB_OK_INFOED.swap(true, Ordering::Relaxed) {
            utils::info!(
                "seed-sync hitbox: reading {} real capsule(s) (array_ptr={:#x}, model_offset={:#x})",
                capsules.len(),
                array_ptr,
                model_offset
            );
        }

        Some(capsules)
    }

    pub(crate) fn body_capsules(&self, target: &Player) -> Vec<(Vec3, Vec3, f32)> {
        if let Some(real) = self.real_hitbox_capsules(target) {
            return real;
        }

        SEED_SYNC_CAPSULES
            .iter()
            .map(|(a, b, r)| {
                let start = target.bone_position(self, a.u64());
                let end = target.bone_position(self, b.u64());
                (start, end, *r)
            })
            .collect()
    }

    pub fn seed_sync_will_hit(&self, local: &Player, target: &Player) -> bool {
        let missing = self.missing_required_offsets();
        if !missing.is_empty() {
            if !UNAVAILABLE_WARNED.swap(true, Ordering::Relaxed) {
                utils::warn!(
                    "seed-sync disabled: {} required offset(s) unresolved: {}",
                    missing.len(),
                    missing.join(", ")
                );
            }
            return true;
        }

        let Some(weapon) = self.active_weapon_entity(local) else {
            return false;
        };

        let vdata_off = self.offsets.seed_sync.weapon_vdata_ptr.unwrap_or(0);
        let vdata_raw: u64 = self.process.read(weapon + vdata_off);
        let Some(vdata) = (vdata_raw >= 0x10000).then_some(vdata_raw) else {
            if !VDATA_WARNED.swap(true, Ordering::Relaxed) {
                utils::warn!(
                    "seed-sync: weapon vdata pointer unreadable (offset {:#x} -> value {:#x}); prediction inactive until it resolves",
                    vdata_off,
                    vdata_raw
                );
            }
            return false;
        };

        if !READY_INFOED.swap(true, Ordering::Relaxed) {
            utils::info!(
                "seed-sync active: vdata @ {:#x} (offset {:#x})",
                vdata,
                vdata_off
            );
        }

        let so = &self.offsets.seed_sync;

        let cmd_angles = local.view_angles(self);
        let tick = self.tick_count();
        let item_def_idx: u16 = self.process.read(
            weapon
                + self.offsets.weapon.attribute_manager
                + self.offsets.weapon.item
                + self.offsets.econ_item_view.item_definition_index,
        );
        let recoil_index: i32 = self.process.read(weapon + so.recoil_index.unwrap_or(0));
        let fire_mode: i32 = self.process.read(weapon + so.weapon_mode.unwrap_or(0));

        let inaccuracy = self.compute_inaccuracy(local.pawn, weapon, vdata);
        let spread = self.firing_mode_float(vdata, so.spread, fire_mode);

        let aim_punch = local.aim_punch(self);
        let view = cmd_angles + aim_punch;
        let (forward, right, up) = angle_vectors(view);
        let eye = local.eye_position(self);

        let capsules = self.body_capsules(target);

        let mut verdict = true;
        let mut missed_at = 0;
        for tick_offset in 0..NEEDED_TICKS {
            let seed = spread_seed(cmd_angles.x, cmd_angles.y, tick - 1 + tick_offset);
            let sv = calculate_spread(
                seed.wrapping_add(1) as i32,
                inaccuracy,
                spread,
                recoil_index as f32,
                item_def_idx,
                0,
            );
            let dir = (forward + right * (-sv.x) + up * sv.y).normalize();

            let mut hit_any = false;
            for &(start, end, radius) in &capsules {
                if ray_hits_capsule(eye, dir, start, end, radius) {
                    hit_any = true;
                    break;
                }
            }

            if !hit_any {
                verdict = false;
                missed_at = tick_offset;
                break;
            }
        }

        let cur = i8::from(verdict);
        if LAST_VERDICT.swap(cur, Ordering::Relaxed) != cur {
            let outcome = if verdict { "HIT" } else { "MISS" };
            utils::info!(
                "seed-sync {outcome}: item={item_def_idx} inaccuracy={inaccuracy:.4} spread={spread:.4} recoil={recoil_index} tick={tick}{}",
                if verdict {
                    String::new()
                } else {
                    format!(" (missed at tick_offset {missed_at})")
                }
            );
        }

        verdict
    }
}
