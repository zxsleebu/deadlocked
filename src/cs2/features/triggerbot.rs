use std::time::{Duration, Instant};

use glam::Vec2;
use rand::rng;

use crate::{
    config::Config,
    cs2::{
        CS2,
        bones::Bones,
        entity::{player::Player, weapon_class::WeaponClass},
    },
    math::angles_to_fov,
    os::mouse::Mouse,
};

#[derive(Debug, Default)]
pub struct Triggerbot {
    shot_start: Option<Instant>,
    shot_end: Option<Instant>,
    pub active: bool,
    last_angles: Option<Vec2>,
}

impl CS2 {
    pub fn triggerbot(&mut self, config: &Config) {
        let hotkey = config.aim.triggerbot_hotkey;
        let config = self.triggerbot_config(config);

        if !config.enabled {
            return;
        }

        if !Self::check_hotkey(&self.input, config.mode, hotkey, &mut self.trigger.active) {
            return;
        }

        if self.trigger.shot_start.is_some() || self.trigger.shot_end.is_some() {
            return;
        }

        let Some(local_player) = Player::local_player(self) else {
            return;
        };

        if config.flash_check && local_player.is_flashed(self) {
            return;
        }

        if !config.force_shoot_when_sure
            && config.scope_check
            && local_player.weapon_class(self) == WeaponClass::Sniper
            && !local_player.is_scoped(self)
        {
            return;
        }

        if !config.force_shoot_when_sure {
            if config.velocity_check
                && local_player.velocity(self).length() > config.velocity_threshold
            {
                return;
            }

            if config.in_air_check && local_player.is_in_air(self) {
                return;
            }
        }

        if config.force_shoot_when_sure {
            let current_angles = local_player.view_angles(self);
            let stable = match self.trigger.last_angles {
                Some(last) => {
                    angles_to_fov(&last, &current_angles) <= config.angle_stability_threshold
                }
                None => false,
            };
            self.trigger.last_angles = Some(current_angles);
            if !stable {
                return;
            }
        }

        if config.force_shoot_when_sure && !config.only_when_aiming {
            let Some(state) = self.seed_sync_prepare(&local_player) else {
                return;
            };
            let is_ffa = self.is_ffa();
            let local_team = local_player.team(self);

            let mut hit_found = false;
            for player in &self.players {
                if !player.is_valid(self) {
                    continue;
                }
                if !is_ffa && player.team(self) == local_team {
                    continue;
                }
                if !player.visible(self, &local_player) {
                    continue;
                }
                if self.seed_sync_check(&state, player, config.head_only, config.ignore_legs) {
                    hit_found = true;
                    break;
                }
            }

            if !hit_found {
                return;
            }

            let now = Instant::now();
            self.trigger.shot_start = Some(now);
            self.trigger.shot_end = Some(now + Duration::from_millis(config.shot_duration));
            return;
        }

        let Some(player) = local_player.crosshair_entity(self) else {
            return;
        };

        if !self.is_ffa() && player.team(self) == local_player.team(self) {
            return;
        }

        if !player.visible(self, &local_player) {
            return;
        }

        if config.head_only && !config.force_shoot_when_sure {
            let head = player.bone_position(self, Bones::Head.u64());

            let target_angle = self.angle_to_target(&local_player, &head, &Vec2::ZERO);
            let view_angles = local_player.view_angles(self);
            let fov = angles_to_fov(&view_angles, &target_angle);

            let head_radius_fov =
                3.5 / (local_player.position(self) - player.position(self)).length() * 100.0;

            if fov > head_radius_fov {
                return;
            }
        }

        if config.force_shoot_when_sure
            && !self.seed_sync_will_hit(
                &local_player,
                &player,
                config.head_only,
                config.ignore_legs,
            )
        {
            return;
        }

        let mean = (*config.delay.start() + *config.delay.end()) as f32 / 2.0;
        let std_dev = (*config.delay.end() - *config.delay.start()) as f32 / 2.0;

        let normal = rand_distr::Normal::new(mean, std_dev).unwrap();
        use rand_distr::Distribution as _;
        let sampled = normal.sample(&mut rng()).max(0.0) as u64;

        let delay_ms = if config.force_shoot_when_sure {
            0
        } else {
            sampled
        };

        let now = Instant::now();
        let delay = Duration::from_millis(delay_ms);
        self.trigger.shot_start = Some(now + delay);
        self.trigger.shot_end = Some(now + delay + Duration::from_millis(config.shot_duration));
    }

    pub fn triggerbot_shoot(&mut self, mouse: &mut Mouse) {
        let now = Instant::now();

        if let Some(shot_time) = self.trigger.shot_start
            && now >= shot_time
        {
            mouse.left_press();
            self.trigger.shot_start = None;
        }

        if let Some(shot_end) = self.trigger.shot_end
            && now >= shot_end
        {
            mouse.left_release();
            self.trigger.shot_end = None;
        }
    }
}
