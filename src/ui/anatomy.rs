//! What a graph view measured while laying itself out: nodes, pins, wires and transforms.

use std::{collections::HashMap, sync::Arc};

use egui::{Color32, Context, Id, LayerId, Order, Pos2, Rect, emath::TSTransform};

use crate::{InPinId, NodeId, OutPinId};

use super::{
    WireStyle,
    cable::{CableAnchor, CableGeometry},
    pin::AnyPin,
    wire::{WirePaint, WirePins},
};

/// The ids one [`Snarl`](crate::Snarl) view gives its layer, nodes, pins, wires and menus.
///
/// Every id is derived from `snarl_id` and from the identity of the thing it names, never from
/// the order widgets happen to be laid out in. Two views with different `snarl_id`s, side by
/// side or one inside a node of the other, therefore never share an id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnarlViewIds {
    /// Root persistent widget id for this graph view.
    pub snarl_id: Id,

    /// Transformed sublayer id where graph contents are painted and interacted with.
    pub layer_id: LayerId,
}

impl SnarlViewIds {
    /// Creates deterministic view identifiers from a resolved `snarl_id` and layer `order`.
    #[must_use]
    #[inline]
    pub fn new(snarl_id: Id, order: Order) -> Self {
        Self {
            snarl_id,
            layer_id: LayerId::new(order, snarl_id),
        }
    }

    /// Persistent scope id for a specific node.
    #[must_use]
    #[inline]
    pub fn node_id(self, node: NodeId) -> Id {
        self.snarl_id.with(("snarl-node", node))
    }

    /// Persistent interaction id for a node's draggable frame.
    #[must_use]
    #[inline]
    pub fn node_frame_id(self, node: NodeId) -> Id {
        self.node_id(node).with("frame")
    }

    /// Persistent interaction id for an input pin handle.
    ///
    /// Keying by `(node, input)` rather than sequential auto-ids ensures pin handles keep
    /// their identity even when widget counts inside `show_input` change dynamically.
    #[must_use]
    #[inline]
    pub fn in_pin_id(self, pin: InPinId) -> Id {
        self.node_id(pin.node).with(("in-pin", pin.input))
    }

    /// Persistent interaction id for an output pin handle.
    #[must_use]
    #[inline]
    pub fn out_pin_id(self, pin: OutPinId) -> Id {
        self.node_id(pin.node).with(("out-pin", pin.output))
    }

    /// Persistent interaction id for a connected wire body.
    #[must_use]
    #[inline]
    pub fn wire_id(self, wire: WirePins) -> Id {
        self.snarl_id.with(("snarl-wire", wire))
    }

    /// Persistent interaction id for a connected wire's midpoint handle.
    #[must_use]
    #[inline]
    pub fn wire_midpoint_id(self, wire: WirePins) -> Id {
        self.snarl_id.with(("snarl-wire-midpoint", wire))
    }

    /// Popup id for the canvas background context menu.
    #[must_use]
    #[inline]
    pub fn graph_menu_popup_id(self) -> Id {
        self.snarl_id.with("graph_menu_popup")
    }

    /// Popup id for the dropped-wire context menu.
    #[must_use]
    #[inline]
    pub fn dropped_wire_popup_id(self) -> Id {
        self.snarl_id.with("dropped_wire_popup")
    }

    /// Popup id for the wire midpoint/context action menu.
    #[must_use]
    #[inline]
    pub fn wire_menu_popup_id(self) -> Id {
        self.snarl_id.with("wire_menu_popup")
    }
}

/// Stable graph-space and screen-space coordinates for inserting nodes from popups or menus.
///
/// Captured when a menu or gesture begins so that subsequent popup relocation near screen edges,
/// search input, or collapsible section clicks never shift the target graph insertion point.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InsertionAnchor {
    /// Target insertion position in graph coordinates.
    pub graph_pos: Pos2,

    /// Unclamped invocation position in global screen coordinates.
    pub screen_pos: Pos2,

    /// Invocation position clamped to the visible clipped canvas rectangle in screen coordinates.
    pub visible_screen_pos: Pos2,
}

/// Coordinate transforms and viewport bounds for a rendered [`Snarl`](crate::Snarl) view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnarlTransform {
    /// Transform from the parent [`egui::Ui`] layer to global screen space (`IDENTITY` at root).
    pub parent_to_global: TSTransform,

    /// Transform from this graph's coordinate space into the parent [`egui::Ui`] layer.
    pub to_parent: TSTransform,

    /// Composed transform from this graph's coordinate space to global screen space.
    pub to_global: TSTransform,

    /// Inverse transform from global screen space to this graph's coordinate space.
    pub from_global: TSTransform,

    /// Allocated canvas content rectangle in parent-layer coordinates.
    pub local_ui_rect: Rect,

    /// Allocated canvas content rectangle in global screen coordinates.
    pub global_ui_rect: Rect,

    /// Visible clipped canvas rectangle in global screen coordinates.
    pub global_clip_rect: Rect,

    /// Visible canvas viewport in graph coordinates.
    pub viewport: Rect,
}

impl SnarlTransform {
    /// Maps a point from graph coordinates to global screen coordinates.
    #[must_use]
    #[inline]
    pub fn graph_to_screen_pos(&self, graph_pos: Pos2) -> Pos2 {
        self.to_global * graph_pos
    }

    /// Maps a point from global screen coordinates to graph coordinates.
    #[must_use]
    #[inline]
    pub fn screen_to_graph_pos(&self, screen_pos: Pos2) -> Pos2 {
        self.from_global * screen_pos
    }

    /// Maps a rectangle from graph coordinates to global screen coordinates.
    #[must_use]
    #[inline]
    pub fn graph_to_screen_rect(&self, graph_rect: Rect) -> Rect {
        self.to_global * graph_rect
    }

    /// Maps a rectangle from global screen coordinates to graph coordinates.
    #[must_use]
    #[inline]
    pub fn screen_to_graph_rect(&self, screen_rect: Rect) -> Rect {
        self.from_global * screen_rect
    }

    /// Effective global scale factor (`graph -> screen`).
    #[must_use]
    #[inline]
    pub const fn scale(&self) -> f32 {
        self.to_global.scaling
    }

    /// View-local scale factor relative to the parent layer (`graph -> parent`).
    #[must_use]
    #[inline]
    pub const fn local_scale(&self) -> f32 {
        self.to_parent.scaling
    }

    /// Clamps a global screen position into the visible clipped canvas area.
    #[must_use]
    #[inline]
    pub fn clamp_screen_to_visible(&self, screen_pos: Pos2) -> Pos2 {
        if self.global_clip_rect.is_positive() {
            self.global_clip_rect.clamp(screen_pos)
        } else if self.global_ui_rect.is_positive() {
            self.global_ui_rect.clamp(screen_pos)
        } else {
            screen_pos
        }
    }

    /// Builds a stable [`InsertionAnchor`] from a global screen-space invocation position.
    #[must_use]
    #[inline]
    pub fn insertion_anchor_from_screen(&self, screen_pos: Pos2) -> InsertionAnchor {
        let visible_screen_pos = self.clamp_screen_to_visible(screen_pos);
        InsertionAnchor {
            graph_pos: self.from_global * visible_screen_pos,
            screen_pos,
            visible_screen_pos,
        }
    }

    /// Builds a stable [`InsertionAnchor`] directly from a graph-space position.
    #[must_use]
    #[inline]
    pub fn insertion_anchor_from_graph(&self, graph_pos: Pos2) -> InsertionAnchor {
        let screen_pos = self.to_global * graph_pos;
        let visible_screen_pos = self.clamp_screen_to_visible(screen_pos);
        InsertionAnchor {
            graph_pos,
            screen_pos,
            visible_screen_pos,
        }
    }
}

/// Measured geometry of one rendered input or output pin for the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredPin {
    /// Pin identifier (input or output).
    pub pin: AnyPin,

    /// Stable interaction widget id of the pin handle.
    pub widget_id: Id,

    /// Measured pin center in graph coordinates.
    pub graph_center: Pos2,

    /// Measured pin center in global screen coordinates.
    pub screen_center: Pos2,

    /// Measured pin handle interaction rectangle in graph coordinates.
    pub graph_rect: Rect,

    /// Measured content row rectangle for this pin in graph coordinates.
    pub row_rect: Rect,

    /// Resolved wire color emitted by this pin.
    pub wire_color: Color32,

    /// Resolved wire style emitted by this pin.
    pub wire_style: WireStyle,
}

/// Measured geometry of one rendered node for the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredNode {
    /// Node identifier.
    pub id: NodeId,

    /// Whether the node is currently open (uncollapsed).
    pub open: bool,

    /// Animated openness factor in `0.0..=1.0`.
    pub openness: f32,

    /// Zero-based index in back-to-front draw order (`0` is backmost).
    pub draw_order_index: usize,

    /// Authored top-left node position in graph coordinates.
    pub pos: Pos2,

    /// Outer rectangle of the node as laid out this pass, in graph coordinates.
    ///
    /// The same rectangle [`SnarlViewer::final_node_rect`](super::SnarlViewer::final_node_rect) receives.
    pub final_rect: Rect,

    /// [`MeasuredNode::final_rect`] in global screen coordinates.
    pub screen_rect: Rect,

    /// Whether the header, a pin row or the body measured a different size this pass than the
    /// last one. The view has already asked for another pass to settle the layout.
    pub remeasured: bool,
}

/// One connected wire as it was routed, painted and hit tested this pass.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredWire {
    /// The pins the wire joins.
    pub pins: WirePins,

    /// The path of the wire in graph coordinates.
    ///
    /// The strokes in [`MeasuredWire::paint`] were laid along exactly these points and the
    /// pointer was tested against exactly these points.
    pub geometry: Arc<CableGeometry>,

    /// How the wire was painted.
    pub paint: WirePaint,

    /// Half-width of the band around the path in which the pointer counted as on the wire,
    /// in graph coordinates.
    pub hit_tolerance: f32,

    /// Whether the pointer is on this wire.
    pub hovered: bool,

    /// Whether this wire is the selected wire of its view.
    pub selected: bool,
}

impl MeasuredWire {
    /// The point halfway along the wire, with the wire's direction there.
    #[must_use]
    #[inline]
    pub fn midpoint(&self) -> CableAnchor {
        self.geometry.midpoint()
    }
}

/// One wire as offered to [`SnarlViewer::wire_paint`](super::SnarlViewer::wire_paint).
#[derive(Clone, Copy, Debug)]
pub struct WireView<'a> {
    /// The pins the wire joins, or `None` for a wire that is still being dragged.
    pub pins: Option<WirePins>,

    /// The path the returned paint will be laid along, in graph coordinates.
    pub geometry: &'a CableGeometry,

    /// Whether the pointer is on this wire.
    pub hovered: bool,

    /// Whether this wire is the selected wire of its view.
    pub selected: bool,

    /// Colour mixed from the two pins.
    pub color: Color32,

    /// Width from [`SnarlStyle::wire_width`](super::SnarlStyle::wire_width), in graph coordinates.
    pub width: f32,

    /// Screen points per graph unit.
    pub scale: f32,
}

/// What was done to a wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WireTrigger {
    /// Primary click or tap on the wire. The view has already selected it.
    Click,

    /// Primary double click or double tap on the wire.
    DoubleClick,

    /// Secondary click on the wire.
    SecondaryClick,

    /// Touch held on the wire.
    LongTouch,

    /// Delete or Backspace pressed while the wire is selected.
    Delete,

    /// Enter or Space pressed while the wire is selected.
    Activate,
}

impl WireTrigger {
    /// Whether this is how a context menu is usually asked for:
    /// secondary click, long touch, or keyboard activation.
    #[must_use]
    #[inline]
    pub const fn asks_for_menu(self) -> bool {
        matches!(
            self,
            Self::SecondaryClick | Self::LongTouch | Self::Activate
        )
    }
}

/// Something done to a wire, as reported to [`SnarlViewer::wire_action`](super::SnarlViewer::wire_action).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireAction {
    /// The wire acted on.
    pub wire: WirePins,

    /// What was done.
    pub trigger: WireTrigger,

    /// Where on the wire it was done, in graph coordinates.
    ///
    /// The point of the wire under the pointer, or its midpoint for keyboard triggers.
    pub anchor: CableAnchor,

    /// [`WireAction::anchor`] as a stable insertion point for a menu or a new node.
    pub insertion: InsertionAnchor,
}

/// Complete measured anatomy snapshot of a [`Snarl`](crate::Snarl) view from its latest pass.
#[derive(Clone, Debug)]
pub struct SnarlAnatomy {
    /// Deterministic widget and popup ids for this view.
    pub view_ids: SnarlViewIds,

    /// Transform and viewport bounds for this view.
    pub transform: SnarlTransform,

    /// Nodes in back-to-front draw order.
    pub nodes: Vec<MeasuredNode>,

    /// Measured input pins keyed by [`InPinId`].
    pub input_pins: HashMap<InPinId, MeasuredPin>,

    /// Measured output pins keyed by [`OutPinId`].
    pub output_pins: HashMap<OutPinId, MeasuredPin>,

    /// Connected wires whose two pins were both shown, ordered by [`WirePins`].
    pub wires: Vec<MeasuredWire>,

    /// Where the open graph, dropped-wire or wire menu inserts, if one is open.
    ///
    /// Captured when the menu opened; it does not follow the popup when egui moves the popup to
    /// keep it on screen.
    pub active_insertion_anchor: Option<InsertionAnchor>,

    /// Nodes selected in this view.
    pub selected_nodes: Vec<NodeId>,

    /// Wire selected in this view, if any.
    pub selected_wire: Option<WirePins>,

    /// Whether this view is shown inside a node of another view.
    pub embedded: bool,

    /// Whether the pointer is over a view embedded in this one, which then owns pan, zoom and
    /// wire hover.
    pub pointer_in_embedded_view: bool,
}

impl SnarlAnatomy {
    /// Looks up measured geometry for `node`.
    #[must_use]
    pub fn node(&self, node: NodeId) -> Option<&MeasuredNode> {
        self.nodes.iter().find(|measured| measured.id == node)
    }

    /// Looks up measured geometry for an input pin.
    #[must_use]
    #[inline]
    pub fn in_pin(&self, pin: InPinId) -> Option<&MeasuredPin> {
        self.input_pins.get(&pin)
    }

    /// Looks up measured geometry for an output pin.
    #[must_use]
    #[inline]
    pub fn out_pin(&self, pin: OutPinId) -> Option<&MeasuredPin> {
        self.output_pins.get(&pin)
    }

    /// Looks up measured geometry for any pin.
    #[must_use]
    #[inline]
    pub fn pin(&self, pin: AnyPin) -> Option<&MeasuredPin> {
        match pin {
            AnyPin::In(id) => self.in_pin(id),
            AnyPin::Out(id) => self.out_pin(id),
        }
    }

    /// Looks up measured geometry for a connected wire.
    #[must_use]
    pub fn wire(&self, pins: WirePins) -> Option<&MeasuredWire> {
        self.wires.iter().find(|wire| wire.pins == pins)
    }

    /// Whether `graph_pos` lies inside the outer rectangle of some node.
    #[must_use]
    pub fn is_occluded_by_node(&self, graph_pos: Pos2) -> bool {
        self.nodes
            .iter()
            .any(|node| node.final_rect.contains(graph_pos))
    }

    /// The wire the pointer is on, if any.
    #[must_use]
    pub fn hovered_wire(&self) -> Option<&MeasuredWire> {
        self.wires.iter().find(|wire| wire.hovered)
    }

    pub(crate) fn store(self, ctx: &Context) -> Arc<Self> {
        let anatomy = Arc::new(self);
        ctx.data_mut(|d| {
            d.insert_temp(anatomy.view_ids.snarl_id, Arc::clone(&anatomy));
        });
        anatomy
    }
}

/// Returns what the graph view with the given id measured in its latest pass.
///
/// `id` is the id given to [`SnarlWidget::id`](super::SnarlWidget::id), or the one found in
/// [`SnarlViewIds::snarl_id`]. Nothing is returned before the view has been shown once.
#[must_use]
#[inline]
pub fn get_snarl_anatomy(id: Id, ctx: &Context) -> Option<Arc<SnarlAnatomy>> {
    ctx.data(|d| d.get_temp::<Arc<SnarlAnatomy>>(id))
}
