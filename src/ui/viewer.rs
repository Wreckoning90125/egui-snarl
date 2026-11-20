use egui::{Painter, Pos2, Rect, Style, Ui, Vec2, emath::TSTransform};

use crate::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};

use super::{
    BackgroundPattern, NodeLayout, SnarlStyle,
    anatomy::{InsertionAnchor, SnarlAnatomy, WireAction, WireTrigger, WireView},
    cable::CableGeometry,
    pin::{AnyPins, SnarlPin},
    wire::{WirePaint, WirePins, WireRoute},
};

/// `SnarlViewer` is a trait for viewing a Snarl.
///
/// It can extract necessary data from the nodes and controls their
/// response to certain events.
pub trait SnarlViewer<T> {
    /// Returns title of the node.
    fn title(&mut self, node: &T) -> String;

    /// Returns the node's frame.
    /// All node's elements will be rendered inside this frame.
    /// Except for pins if they are configured to be rendered outside of the frame.
    ///
    /// Returns `default` by default.
    /// `default` frame is taken from the [`SnarlStyle::node_frame`] or constructed if it's `None`.
    ///
    /// Override this method to customize the frame for specific nodes.
    fn node_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<T>,
    ) -> egui::Frame {
        let _ = (node, inputs, outputs, snarl);
        default
    }

    /// Returns the node's header frame.
    ///
    /// This frame would be placed on top of the node's frame.
    /// And header UI (see [`show_header`]) will be placed inside this frame.
    ///
    /// Returns `default` by default.
    /// `default` frame is taken from the [`SnarlStyle::header_frame`],
    /// or [`SnarlStyle::node_frame`] with removed shadow if `None`,
    /// or constructed if both are `None`.
    fn header_frame(
        &mut self,
        default: egui::Frame,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<T>,
    ) -> egui::Frame {
        let _ = (node, inputs, outputs, snarl);
        default
    }
    /// Checks if node has a custom egui style.
    #[inline]
    fn has_node_style(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<T>,
    ) -> bool {
        let _ = (node, inputs, outputs, snarl);
        false
    }

    /// Modifies the node's egui style
    fn apply_node_style(
        &mut self,
        style: &mut Style,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<T>,
    ) {
        let _ = (style, node, inputs, outputs, snarl);
    }

    /// Returns elements layout for the node.
    ///
    /// Node consists of 5 parts: header, body, footer, input pins and output pins.
    /// See [`NodeLayout`] for available placements.
    ///
    /// Returns `default` by default.
    /// `default` layout is taken from the [`SnarlStyle::node_layout`] or constructed if it's `None`.
    /// Override this method to customize the layout for specific nodes.
    #[inline]
    fn node_layout(
        &mut self,
        default: NodeLayout,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        snarl: &Snarl<T>,
    ) -> NodeLayout {
        let _ = (node, inputs, outputs, snarl);
        default
    }

    /// Renders elements inside the node's header frame.
    ///
    /// This is the good place to show the node's title and controls related to the whole node.
    ///
    /// By default it shows the node's title.
    #[inline]
    fn show_header(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (inputs, outputs);
        ui.label(self.title(&snarl[node]));
    }

    /// Returns number of input pins of the node.
    ///
    /// [`SnarlViewer::show_input`] will be called for each input in range `0..inputs()`.
    fn inputs(&mut self, node: &T) -> usize;

    /// Renders one specified node's input element and returns drawer for the corresponding pin.
    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) -> impl SnarlPin + 'static;

    /// Returns number of output pins of the node.
    ///
    /// [`SnarlViewer::show_output`] will be called for each output in range `0..outputs()`.
    fn outputs(&mut self, node: &T) -> usize;

    /// Renders the node's output.
    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) -> impl SnarlPin + 'static;

    /// Checks if node has something to show in body - between input and output pins.
    #[inline]
    fn has_body(&mut self, node: &T) -> bool {
        let _ = node;
        false
    }

    /// Renders the node's body.
    #[inline]
    fn show_body(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (node, inputs, outputs, ui, snarl);
    }

    /// Checks if node has something to show in footer - below pins and body.
    #[inline]
    fn has_footer(&mut self, node: &T) -> bool {
        let _ = node;
        false
    }

    /// Renders the node's footer.
    #[inline]
    fn show_footer(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (node, inputs, outputs, ui, snarl);
    }

    /// Reports the final node's rect after rendering.
    ///
    /// It aimed to be used for custom positioning of nodes that requires node dimensions for calculations.
    /// Node's position can be modified directly in this method.
    #[inline]
    fn final_node_rect(&mut self, node: NodeId, rect: Rect, ui: &mut Ui, snarl: &mut Snarl<T>) {
        let _ = (node, rect, ui, snarl);
    }

    /// Checks if node has something to show in on-hover popup.
    #[inline]
    fn has_on_hover_popup(&mut self, node: &T) -> bool {
        let _ = node;
        false
    }

    /// Renders the node's on-hover popup.
    #[inline]
    fn show_on_hover_popup(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (node, inputs, outputs, ui, snarl);
    }

    /// Checks if wire has something to show in widget.
    /// This may not be called if wire is invisible.
    ///
    /// The widget is laid out centred on the midpoint of the wire's path.
    /// It is left out while it would cover either pin of the wire, and while a wire is being dragged.
    #[inline]
    fn has_wire_widget(&mut self, from: &OutPinId, to: &InPinId, snarl: &Snarl<T>) -> bool {
        let _ = (from, to, snarl);
        false
    }

    /// Renders the wire's widget.
    /// This may not be called if wire is invisible.
    #[inline]
    fn show_wire_widget(&mut self, from: &OutPin, to: &InPin, ui: &mut Ui, snarl: &mut Snarl<T>) {
        let _ = (from, to, ui, snarl);
    }

    /// Lays out the path of a wire.
    ///
    /// Return `None` to use [`WireRoute::default_route`], which follows the wire style, slack and sway.
    /// A returned path is used as is for painting, hit testing, bounds and anchors.
    /// It should start at `route.from` and end at `route.to`; those are the measured pin centres.
    ///
    /// Called for every visible wire on every pass, so keep it cheap or cache by `route`.
    #[inline]
    fn wire_route(&mut self, route: &WireRoute, snarl: &Snarl<T>) -> Option<CableGeometry> {
        let _ = (route, snarl);
        None
    }

    /// Decides how a wire is painted along its path.
    ///
    /// `default` is one stroke of the mixed pin colour, half again as wide while the wire is
    /// hovered or selected. Return it to keep that look, or build a [`WirePaint`] from layered
    /// strokes: a casing, a core, a glow. Discs added with [`WirePaint::with_bead`] ride on the
    /// same path. Return [`WirePaint::hidden`] to paint the wire yourself.
    ///
    /// Called once for every visible wire on every pass with `wire.hovered` false to learn how
    /// wide the wire is, and once more for the wire the pointer turns out to be on.
    #[inline]
    fn wire_paint(
        &mut self,
        wire: &WireView<'_>,
        default: WirePaint,
        snarl: &Snarl<T>,
    ) -> WirePaint {
        let _ = (wire, snarl);
        default
    }

    /// Reacts to something done to a wire with the pointer, a touch or the keyboard.
    ///
    /// Not called for triggers that open the wire menu when [`SnarlViewer::has_wire_menu`]
    /// returns `true` for the wire.
    ///
    /// By default a secondary click, and Delete or Backspace on the selected wire, ask
    /// [`SnarlViewer::disconnect`] to remove the wire. Everything else does nothing.
    ///
    /// Dragging from a pin to reconnect, dragging a node and panning the view are never
    /// reported here; they stay with the pins, the nodes and the view.
    #[inline]
    fn wire_action(&mut self, action: &WireAction, ui: &mut Ui, snarl: &mut Snarl<T>) {
        let _ = ui;
        if matches!(
            action.trigger,
            WireTrigger::SecondaryClick | WireTrigger::Delete
        ) {
            let from = OutPin::new(snarl, action.wire.out_pin);
            let to = InPin::new(snarl, action.wire.in_pin);
            self.disconnect(&from, &to, snarl);
        }
    }

    /// Checks if the wire has a context menu.
    ///
    /// When it does, a secondary click or long touch on the wire, and Enter or Space on the
    /// selected wire, open that menu instead of being reported to [`SnarlViewer::wire_action`].
    #[inline]
    fn has_wire_menu(&mut self, wire: WirePins, snarl: &mut Snarl<T>) -> bool {
        let _ = (wire, snarl);
        false
    }

    /// Shows the context menu of a wire.
    ///
    /// `anchor` is the point of the wire the menu was opened at: under the pointer, or the
    /// midpoint when opened from the keyboard. It stays put while the menu is open, wherever
    /// the popup itself ends up on screen, so it is safe to insert a node there.
    #[inline]
    fn show_wire_menu(
        &mut self,
        wire: WirePins,
        anchor: InsertionAnchor,
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (wire, anchor, ui, snarl);
    }

    /// Reports what the view measured this pass, after everything has been laid out and painted.
    ///
    /// `ui` paints into the graph layer, in graph coordinates, above nodes and wires.
    /// The same data is available afterwards from [`get_snarl_anatomy`](super::get_snarl_anatomy).
    #[inline]
    fn measured(&mut self, anatomy: &SnarlAnatomy, ui: &mut Ui, snarl: &mut Snarl<T>) {
        let _ = (anatomy, ui, snarl);
    }

    /// Checks if the snarl has something to show in context menu if right-clicked or long-touched on empty space at `pos`.
    #[inline]
    fn has_graph_menu(&mut self, pos: Pos2, snarl: &mut Snarl<T>) -> bool {
        let _ = (pos, snarl);
        false
    }

    /// Show context menu for the snarl.
    ///
    /// This can be used to implement menu for adding new nodes.
    ///
    /// `pos` is the point of the graph the menu was opened at. It stays put while the menu is
    /// open, even when the popup is moved to fit on screen.
    #[inline]
    fn show_graph_menu(
        &mut self,
        pos: Pos2,
        selected_nodes: &[NodeId],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (pos, selected_nodes, ui, snarl);
    }

    /// Checks if the snarl has something to show in context menu if wire drag is stopped at `pos`.
    #[inline]
    fn has_dropped_wire_menu(&mut self, src_pins: AnyPins, snarl: &mut Snarl<T>) -> bool {
        let _ = (src_pins, snarl);
        false
    }

    /// Show context menu for the snarl. This menu is opened when releasing a pin to empty
    /// space. It can be used to implement menu for adding new node, and directly
    /// connecting it to the released wire.
    #[inline]
    fn show_dropped_wire_menu(
        &mut self,
        pos: Pos2,
        ui: &mut Ui,
        src_pins: AnyPins,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (pos, ui, src_pins, snarl);
    }

    /// Checks if the node has something to show in context menu if right-clicked or long-touched on the node.
    #[inline]
    fn has_node_menu(&mut self, node: &T) -> bool {
        let _ = node;
        false
    }

    /// Show context menu for the snarl.
    ///
    /// This can be used to implement menu for adding new nodes.
    #[inline]
    fn show_node_menu(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        selected_nodes: &[NodeId],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (node, inputs, outputs, selected_nodes, ui, snarl);
    }

    /// Asks the viewer to connect two pins.
    ///
    /// This is usually happens when user drags a wire from one node's output pin to another node's input pin or vice versa.
    /// By default this method connects the pins and returns `Ok(())`.
    #[inline]
    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<T>) {
        snarl.connect(from.id, to.id);
    }

    /// Asks the viewer to disconnect two pins.
    #[inline]
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<T>) {
        snarl.disconnect(from.id, to.id);
    }

    /// Asks the viewer to disconnect all wires from the output pin.
    ///
    /// This is usually happens when right-clicking on an output pin.
    /// By default this method disconnects the pins and returns `Ok(())`.
    #[inline]
    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<T>) {
        snarl.drop_outputs(pin.id);
    }

    /// Asks the viewer to disconnect all wires from the input pin.
    ///
    /// This is usually happens when right-clicking on an input pin.
    /// By default this method disconnects the pins and returns `Ok(())`.
    #[inline]
    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<T>) {
        snarl.drop_inputs(pin.id);
    }

    /// Draws background of the snarl view.
    ///
    /// By default it draws the background pattern using [`BackgroundPattern::draw`].
    ///
    /// If you want to draw the background yourself, you can override this method.
    #[inline]
    fn draw_background(
        &mut self,
        background: Option<&BackgroundPattern>,
        viewport: &Rect,
        snarl_style: &SnarlStyle,
        style: &Style,
        painter: &Painter,
        snarl: &Snarl<T>,
    ) {
        let _ = snarl;

        if let Some(background) = background {
            background.draw(viewport, snarl_style, style, painter);
        }
    }

    /// Informs the viewer that nodes were moved.
    ///
    /// Called each frame during a drag with the frame's `delta`, after Snarl node positions were updated.
    /// When `drag_finished` is `true`, this is the final call for this drag gesture.
    #[inline]
    fn nodes_moved(
        &mut self,
        nodes: &[NodeId],
        delta: Vec2,
        drag_finished: bool,
        snarl: &mut Snarl<T>,
    ) {
        let _ = (nodes, delta, drag_finished, snarl);
    }

    /// Informs the viewer what is the current transform of the snarl view
    /// and allows viewer to override it.
    ///
    /// This method is called in the beginning of the graph rendering.
    ///
    /// By default it does nothing.
    #[inline]
    fn current_transform(&mut self, to_global: &mut TSTransform, snarl: &mut Snarl<T>) {
        let _ = (to_global, snarl);
    }

    /// Close behavior for context menus (graph, node, dropped-wire).
    /// Default is `CloseOnClick` (compatible with old behavior).
    /// Override to `CloseOnClickOutside` if your menu has interactive widgets
    /// (search fields, collapsible sections, sliders).
    fn menu_close_behavior(&self) -> egui::PopupCloseBehavior {
        egui::PopupCloseBehavior::CloseOnClick
    }

    /// Asks the viewer if the viewed area should be centered to contain all nodes.
    #[inline]
    fn center_view(&mut self, _snarl: &mut Snarl<T>) -> bool {
        false
    }
    /// Asks the viewer for updated list of selected nodes.
    /// List of currently selected nodes is provided as `currently_selected`.
    /// If viewer returns `Some(nodes)`, those nodes will be selected instead of the currently selected nodes.
    /// 'ui' is also provided to the user, in order to enable keyboard control over selection.
    #[inline]
    fn selected_nodes(
        &mut self,
        currently_selected: &[NodeId],
        ui: &mut Ui,
        snarl: &mut Snarl<T>,
    ) -> Option<Vec<NodeId>> {
        let _ = (currently_selected, ui, snarl);
        None
    }
}
