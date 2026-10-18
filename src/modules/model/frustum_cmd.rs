// FRUSTUM: truncated cone (circular or elliptical) built on the same safe
// kernel path as CYLINDER/CONE (`solid_model::cone_frustum_solid` + `placed`).
// Every numeric input is validated; a failed build returns to the current
// prompt instead of committing anything, so the command can never crash.

use codec::entities::Solid3D;
use codec::objects::SolidHistoryOperation;
use codec::EntityType;
use glam::DVec3;
use kernel::brep::Body;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::solid_model;
use crate::scene::model::solid_model::cone_frustum_solid;
use crate::t;

#[derive(Clone, Copy)]
enum Step {
    BaseCenter,
    BaseRadius,
    TopRadius,
    Height,
}

#[derive(Clone, Copy)]
struct Defaults {
    base_radius: f64,
    top_radius: f64,
    height: f64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            base_radius: 5.0,
            top_radius: 2.5,
            height: 10.0,
        }
    }
}

fn defaults() -> &'static std::sync::Mutex<Defaults> {
    static DEFAULTS: std::sync::OnceLock<std::sync::Mutex<Defaults>> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| std::sync::Mutex::new(Defaults::default()))
}

pub struct FrustumCommand {
    step: Step,
    frame: Option<WorkingPlane>,
    base_radius: f64,
    top_radius: f64,
    remembered: Defaults,
    plane: WorkingPlane,
}

impl FrustumCommand {
    pub fn new() -> Self {
        let remembered = defaults().lock().map(|value| *value).unwrap_or_default();
        Self {
            step: Step::BaseCenter,
            frame: None,
            base_radius: remembered.base_radius,
            top_radius: remembered.top_radius,
            remembered,
            plane: WorkingPlane::default(),
        }
    }

    fn frame_at(&self, center: DVec3) -> WorkingPlane {
        WorkingPlane {
            origin: center,
            x: self.plane.x,
            y: self.plane.y,
            z: self.plane.z,
        }
    }

    fn history_transform(frame: WorkingPlane) -> [f64; 16] {
        glam::DMat4::from_cols(
            frame.x.extend(0.0),
            frame.y.extend(0.0),
            frame.z.extend(0.0),
            frame.origin.extend(1.0),
        )
        .to_cols_array()
    }

    /// Validate radii/height and build the placed body plus its history op.
    /// Returns `None` for degenerate input; callers surface that as a
    /// "try again" prompt rather than an error state.
    fn build(&self, height: f64) -> Option<(Body, SolidHistoryOperation)> {
        let frame = self.frame?;
        if !height.is_finite() || height.abs() < 1e-6 {
            return None;
        }
        let height = height.abs();
        let base = self.base_radius;
        let top = self.top_radius;
        if !base.is_finite() || !top.is_finite() || base < 1e-6 || top < 0.0 {
            return None;
        }
        // A zero top radius is a cone; keep it slightly above zero so the
        // frustum builder always receives a valid two-cap shape.
        let top = top.max(1e-6);
        let local = cone_frustum_solid([0.0; 3], base, base, top, height)?;
        let placed = solid_model::placed(
            &local,
            frame.x.to_array(),
            frame.y.to_array(),
            frame.z.to_array(),
            frame.origin.to_array(),
        )?;
        Some((
            placed,
            crate::scene::model::solid_history::cone_op(
                Self::history_transform(frame),
                base,
                base,
                top,
                height,
            ),
        ))
    }

    fn commit_height(&mut self, height: f64) -> CmdResult {
        let Some((solid, history)) = self.build(height) else {
            return CmdResult::NeedPoint;
        };
        if let Ok(mut remembered) = defaults().lock() {
            *remembered = Defaults {
                base_radius: self.base_radius,
                top_radius: self.top_radius,
                height,
            };
        }
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&solid) else {
            return CmdResult::ReportError(
                t!("FRUSTUM  The solid could not be encoded; nothing was created.").into_owned(),
            );
        };
        let mut entity = Solid3D::new();
        entity.set_sat_document(&document);
        entity.wires = solid_model::edge_wires(&solid);
        CmdResult::CommitSolid {
            entity: EntityType::Solid3D(entity),
            solid: Box::new(solid),
            history,
            erase_source: None,
        }
    }

    fn positive_number(raw: &str) -> Option<f64> {
        let value = crate::entities::common::parse_typed_length(raw.trim())?;
        value.is_finite().then_some(value)
    }

    fn set_base_radius(&mut self, radius: f64) -> CmdResult {
        let Some(radius) = Self::positive_number_owned(radius) else {
            return CmdResult::NeedPoint;
        };
        let Some(frame) = self.frame else {
            return CmdResult::NeedPoint;
        };
        self.base_radius = radius;
        self.top_radius = radius * 0.5;
        let _ = frame;
        self.step = Step::TopRadius;
        CmdResult::NeedPoint
    }

    fn positive_number_owned(radius: f64) -> Option<f64> {
        (radius.is_finite() && radius >= 1e-6).then_some(radius)
    }
}

impl CadCommand for FrustumCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "FRUSTUM"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::BaseCenter => {
                t!("FRUSTUM  Specify center point of base:").into_owned()
            }
            Step::BaseRadius => crate::tf!(
                "FRUSTUM  Specify base radius <{:.4}>:",
                self.remembered.base_radius
            )
            .into_owned(),
            Step::TopRadius => crate::tf!(
                "FRUSTUM  Specify top radius <{:.4}>:",
                self.remembered.top_radius
            )
            .into_owned(),
            Step::Height => crate::tf!(
                "FRUSTUM  Specify height <{:.4}>:",
                self.remembered.height
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        Vec::new()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !point.is_finite() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            Step::BaseCenter => {
                self.frame = Some(self.frame_at(point));
                self.step = Step::BaseRadius;
                CmdResult::NeedPoint
            }
            Step::BaseRadius => {
                let Some(frame) = self.frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                self.set_base_radius(radius)
            }
            Step::TopRadius => {
                let Some(frame) = self.frame else {
                    return CmdResult::NeedPoint;
                };
                let local = frame.to_local(point);
                let radius = local.x.hypot(local.y);
                if radius.is_finite() {
                    self.top_radius = radius;
                }
                self.step = Step::Height;
                CmdResult::NeedPoint
            }
            Step::Height => {
                let Some(frame) = self.frame else {
                    return CmdResult::NeedPoint;
                };
                let height = (point - frame.origin).dot(frame.z);
                self.commit_height(height)
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, raw: &str) -> Option<CmdResult> {
        let token = raw.trim();
        if token.is_empty() {
            return None;
        }
        let number = Self::positive_number(token)?;
        let result = match self.step {
            Step::BaseCenter => return None,
            Step::BaseRadius => self.set_base_radius(number),
            Step::TopRadius => {
                self.top_radius = number;
                self.step = Step::Height;
                CmdResult::NeedPoint
            }
            Step::Height => self.commit_height(number),
        };
        Some(result)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::BaseCenter => CmdResult::Cancel,
            Step::BaseRadius => {
                let remembered = self.remembered.base_radius;
                self.set_base_radius(remembered)
            }
            Step::TopRadius => {
                self.top_radius = self.remembered.top_radius;
                self.step = Step::Height;
                CmdResult::NeedPoint
            }
            Step::Height => {
                let height = self.remembered.height;
                self.commit_height(height)
            }
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["FRUSTUM"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_workflow_commits_solid() {
        if let Ok(mut defs) = defaults().lock() {
            *defs = Defaults::default();
        }
        let mut command = FrustumCommand::new();
        assert!(matches!(
            command.on_point(DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_point(DVec3::new(5.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_point(DVec3::new(2.0, 0.0, 0.0)),
            CmdResult::NeedPoint
        ));
        let result = command.on_text_input("12");
        let Some(CmdResult::CommitSolid { history, .. }) = result else {
            panic!("expected CommitSolid, got {:?}", matches!(result, Some(CmdResult::CommitSolid { .. })));
        };
        let SolidHistoryOperation::Cone(cone) = history else {
            panic!("expected Cone history");
        };
        assert_eq!(cone.height, 12.0);
        assert_eq!(defaults().lock().unwrap().height, 12.0);
    }

    #[test]
    fn degenerate_inputs_never_commit() {
        let mut command = FrustumCommand::new();
        command.on_point(DVec3::ZERO);
        // Zero base radius must not advance via text (rejected).
        assert_eq!(
            command.on_text_input("0"),
            None,
            "zero radius token rejected"
        );
        command.on_point(DVec3::new(4.0, 0.0, 0.0));
        command.on_point(DVec3::new(1.0, 0.0, 0.0));
        // Zero height stays at the height prompt.
        let res = command.on_point(DVec3::new(0.0, 0.0, 0.0));
        assert!(matches!(res, CmdResult::NeedPoint));
        let res = command.on_text_input("-5");
        assert!(res.is_none() || matches!(res, Some(CmdResult::NeedPoint)));
    }

    #[test]
    fn enter_accepts_remembered_defaults() {
        let mut command = FrustumCommand::new();
        command.on_point(DVec3::ZERO);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        let result = command.on_enter();
        assert!(
            matches!(result, CmdResult::CommitSolid { .. }),
            "enter chain should commit with remembered values"
        );
    }
}
