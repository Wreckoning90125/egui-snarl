//! Bookkeeping that lets a graph view shown inside a node of another view own its own input.

use egui::{Context, Id, LayerId, Pos2, Rect};
use smallvec::SmallVec;

type Rects = SmallVec<[Rect; 2]>;

/// The layer a view's own layer hangs under, recorded while the view is shown.
#[derive(Clone, Copy)]
struct ViewLayer {
    root: LayerId,
    pass: u64,
}

/// Screen rectangles of the views shown inside a view.
#[derive(Clone, Default)]
struct EmbeddedRects {
    pass: u64,

    /// Claimed during the pass in progress.
    current: Rects,

    /// Claimed during the pass before it. This is what the host view acts on, because it
    /// handles its input before the views inside it are shown.
    previous: Rects,
}

/// Where a view sits among the graph views shown this pass.
pub struct ViewHost {
    /// Layer the view's own layer must be made a sublayer of.
    ///
    /// egui orders sublayers one level deep, so a view inside another view hangs under the
    /// same layer as its host instead of under its host's layer.
    pub root_layer: LayerId,

    /// Id of the view this one is shown inside, if any.
    pub parent_view: Option<Id>,

    embedded: Rects,
}

impl ViewHost {
    /// Records that the view `snarl_id` is being shown from a [`egui::Ui`] on `parent_layer`.
    pub fn enter(ctx: &Context, snarl_id: Id, parent_layer: LayerId) -> Self {
        let pass = ctx.cumulative_pass_nr();

        ctx.data_mut(|data| {
            // A host registers itself before the views inside it are shown, in the same pass.
            let host = data
                .get_temp::<ViewLayer>(parent_layer.id)
                .filter(|host| host.pass == pass && parent_layer.id != snarl_id);

            let root_layer = host.map_or(parent_layer, |host| host.root);

            data.insert_temp(
                snarl_id,
                ViewLayer {
                    root: root_layer,
                    pass,
                },
            );

            let rects = data.get_temp_mut_or_default::<EmbeddedRects>(snarl_id);
            if rects.pass != pass {
                rects.previous = std::mem::take(&mut rects.current);
                rects.pass = pass;
            }

            Self {
                root_layer,
                parent_view: host.map(|_| parent_layer.id),
                embedded: rects.previous.clone(),
            }
        })
    }

    /// Whether `pointer`, in screen coordinates, is over a view shown inside this one.
    pub fn pointer_in_embedded_view(&self, pointer: Option<Pos2>) -> bool {
        pointer.is_some_and(|pointer| self.embedded.iter().any(|rect| rect.contains(pointer)))
    }

    /// Tells the host view which part of the screen this view covers.
    pub fn claim(&self, ctx: &Context, visible_screen_rect: Rect) {
        let Some(parent_view) = self.parent_view else {
            return;
        };

        if !visible_screen_rect.is_positive() {
            return;
        }

        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<EmbeddedRects>(parent_view)
                .current
                .push(visible_screen_rect);
        });
    }
}
