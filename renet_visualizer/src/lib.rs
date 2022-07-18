use std::collections::HashMap;

use egui::{
    epaint::{PathShape, RectShape},
    pos2, remap, vec2, Color32, Rect, Rgba, RichText, Rounding, Sense, Shape, Stroke, TextStyle, Vec2, WidgetText,
};
use renet::{CircularBuffer, NetworkInfo, RenetServer};

/// Egui visualizer for the renet client. Draws graphs with metrics:
/// RTT, Packet Loss, Kbps Sent/Received.
///
/// N: determines how many values are shown in the graph.
/// 200 is a good value, if updated at 60 fps the graphs would hold 3 seconds of data.
pub struct RenetClientVisualizer<const N: usize> {
    rtt: CircularBuffer<N, f32>,
    sent_bandwidth_kbps: CircularBuffer<N, f32>,
    received_bandwidth_kbps: CircularBuffer<N, f32>,
    packet_loss: CircularBuffer<N, f32>,
    style: RenetVisualizerStyle,
}

/// Egui visualizer for the renet server. Draws graphs for each connected client with metrics:
/// RTT, Packet Loss, Kbps Sent/Received.
///
/// N: determines how many values are shown in the graph.
/// 200 is a good value, if updated at 60 fps the graphs would hold 3 seconds of data.
pub struct RenetServerVisualizer<const N: usize> {
    show_all_clients: bool,
    selected_client: Option<u64>,
    clients: HashMap<u64, RenetClientVisualizer<N>>,
    style: RenetVisualizerStyle,
}

/// Style configuration for the visualizer. Customize size, color and line width.
#[derive(Debug, Clone)]
pub struct RenetVisualizerStyle {
    pub width: f32,
    pub height: f32,
    pub text_color: Color32,
    pub rectangle_stroke: Stroke,
    pub line_stroke: Stroke,
}

enum TopValue {
    SuggestedValues([f32; 5]),
    MaxValue { multiplicated: f32 },
}

enum TextFormat {
    Percentage,
    Normal,
}

impl Default for RenetVisualizerStyle {
    fn default() -> Self {
        Self {
            width: 200.,
            height: 100.,
            text_color: Color32::WHITE,
            rectangle_stroke: Stroke::new(1., Color32::WHITE),
            line_stroke: Stroke::new(1., Color32::WHITE),
        }
    }
}

impl<const N: usize> Default for RenetClientVisualizer<N> {
    fn default() -> Self {
        RenetClientVisualizer::new(RenetVisualizerStyle::default())
    }
}

impl<const N: usize> Default for RenetServerVisualizer<N> {
    fn default() -> Self {
        RenetServerVisualizer::new(RenetVisualizerStyle::default())
    }
}

impl<const N: usize> RenetClientVisualizer<N> {
    pub fn new(style: RenetVisualizerStyle) -> Self {
        Self {
            rtt: CircularBuffer::default(),
            sent_bandwidth_kbps: CircularBuffer::default(),
            received_bandwidth_kbps: CircularBuffer::default(),
            packet_loss: CircularBuffer::default(),
            style,
        }
    }

    /// Add the network information from the client. Should be called every time the client
    /// updates.
    ///
    /// # Usage
    /// ```
    /// # use renet::RenetClient;
    /// # use renet_visualizer::RenetClientVisualizer;
    /// # let mut client = RenetClient::__test();
    /// # let delta = std::time::Duration::ZERO;
    /// # let mut visualizer = RenetClientVisualizer::<5>::new(Default::default());
    /// client.update(delta).unwrap();
    /// visualizer.add_network_info(client.network_info());
    /// ```
    pub fn add_network_info(&mut self, network_info: NetworkInfo) {
        self.rtt.push(network_info.rtt);
        self.sent_bandwidth_kbps.push(network_info.sent_kbps);
        self.received_bandwidth_kbps.push(network_info.received_kbps);
        self.packet_loss.push(network_info.packet_loss);
    }

    /// Renders a new window with all the graphs metrics drawn.
    pub fn show_window(&self, ctx: &egui::Context) {
        egui::Window::new("Client Network Info")
            .resizable(false)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    self.draw_all(ui);
                });
            });
    }

    /// Draws only the Received Kbps metric.
    pub fn draw_received_kbps(&self, ui: &mut egui::Ui) {
        show_graph(
            ui,
            &self.style,
            "Received Kbps",
            TextFormat::Normal,
            TopValue::MaxValue { multiplicated: 1.5 },
            self.received_bandwidth_kbps.as_vec(),
        );
    }

    /// Draws only the Sent Kbps metric.
    pub fn draw_sent_kbps(&self, ui: &mut egui::Ui) {
        show_graph(
            ui,
            &self.style,
            "Sent Kbps",
            TextFormat::Normal,
            TopValue::MaxValue { multiplicated: 1.5 },
            self.sent_bandwidth_kbps.as_vec(),
        );
    }

    /// Draws only the Packet Loss metric.
    pub fn draw_packet_loss(&self, ui: &mut egui::Ui) {
        show_graph(
            ui,
            &self.style,
            "Packet Loss",
            TextFormat::Percentage,
            TopValue::SuggestedValues([0.05, 0.1, 0.25, 0.5, 1.]),
            self.packet_loss.as_vec(),
        );
    }

    /// Draws only the Round Time Trip metric.
    pub fn draw_rtt(&self, ui: &mut egui::Ui) {
        show_graph(
            ui,
            &self.style,
            "Round Time Trip (ms)",
            TextFormat::Normal,
            TopValue::SuggestedValues([32., 64., 128., 256., 512.]),
            self.rtt.as_vec(),
        );
    }

    /// Draw all metrics without a window or layout.
    pub fn draw_all(&self, ui: &mut egui::Ui) {
        self.draw_received_kbps(ui);
        self.draw_sent_kbps(ui);
        self.draw_rtt(ui);
        self.draw_packet_loss(ui);
    }
}

impl<const N: usize> RenetServerVisualizer<N> {
    pub fn new(style: RenetVisualizerStyle) -> Self {
        Self {
            show_all_clients: false,
            selected_client: None,
            clients: HashMap::new(),
            style,
        }
    }

    /// Add a new client to keep track off. Should be called whenever a new client
    /// connected event is received.
    ///
    /// # Usage
    /// ```
    /// # use renet::{RenetServer, ServerEvent};
    /// # use renet_visualizer::RenetServerVisualizer;
    /// # let mut renet_server = RenetServer::__test();
    /// # let mut visualizer = RenetServerVisualizer::<5>::new(Default::default());
    /// while let Some(event) = renet_server.get_event() {
    ///     match event {
    ///         ServerEvent::ClientConnected(client_id, user_data) => {
    ///             visualizer.add_client(client_id);
    ///             // ...
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub fn add_client(&mut self, client_id: u64) {
        self.clients.insert(client_id, RenetClientVisualizer::new(self.style.clone()));
    }

    /// Remove a client from the visualizer. Should be called whenever a client
    /// disconnected event is received.
    ///
    /// # Usage
    /// ```
    /// # use renet::{RenetServer, ServerEvent};
    /// # use renet_visualizer::RenetServerVisualizer;
    /// # let mut renet_server = RenetServer::__test();
    /// # let mut visualizer = RenetServerVisualizer::<5>::new(Default::default());
    /// while let Some(event) = renet_server.get_event() {
    ///     match event {
    ///         ServerEvent::ClientDisconnected(client_id) => {
    ///             visualizer.remove_client(client_id);
    ///             // ...
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    pub fn remove_client(&mut self, client_id: u64) {
        self.clients.remove(&client_id);
    }

    fn add_network_info(&mut self, client_id: u64, network_info: NetworkInfo) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.add_network_info(network_info);
        }
    }

    /// Update the metrics for all connected clients. Should be called every time the server
    /// updates.
    ///
    /// # Usage
    /// ```
    /// # use renet::RenetServer;
    /// # use renet_visualizer::RenetServerVisualizer;
    /// # let mut renet_server = RenetServer::__test();
    /// # let mut visualizer = RenetServerVisualizer::<5>::new(Default::default());
    /// # let delta = std::time::Duration::ZERO;
    /// renet_server.update(delta).unwrap();
    /// visualizer.update(&renet_server);
    /// ```
    pub fn update(&mut self, server: &RenetServer) {
        for client_id in server.clients_id().into_iter() {
            if let Some(network_info) = server.network_info(client_id) {
                self.add_network_info(client_id, network_info);
            }
        }
    }

    /// Draw all metrics without a window or layout for the specified client.
    pub fn draw_client_metrics(&self, client_id: u64, ui: &mut egui::Ui) {
        if let Some(client) = self.clients.get(&client_id) {
            client.draw_all(ui);
        }
    }

    /// Renders a new window with all the graphs metrics drawn. You can choose to show metrics for
    /// all connected clients or for only one chosen by a dropdown.
    pub fn show_window(&mut self, ctx: &egui::Context) {
        egui::Window::new("Server Network Info")
            .resizable(false)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.show_all_clients, "Show all clients");
                    ui.add_enabled_ui(!self.show_all_clients, |ui| {
                        let selected_text = match self.selected_client {
                            Some(client_id) => format!("{}", client_id),
                            None => "------".to_string(),
                        };
                        egui::ComboBox::from_label("Select client")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for client_id in self.clients.keys() {
                                    ui.selectable_value(&mut self.selected_client, Some(*client_id), format!("{}", client_id));
                                }
                            })
                    });
                });
                ui.vertical(|ui| {
                    if self.show_all_clients {
                        for (client_id, client) in self.clients.iter() {
                            ui.vertical(|ui| {
                                ui.heading(format!("Client {}", client_id));
                                ui.horizontal(|ui| {
                                    client.draw_all(ui);
                                });
                            });
                        }
                    } else if let Some(selected_client) = self.selected_client {
                        if let Some(client) = self.clients.get(&selected_client) {
                            ui.horizontal(|ui| {
                                client.draw_all(ui);
                            });
                        }
                    }
                });
            });
    }
}

fn show_graph(
    ui: &mut egui::Ui,
    style: &RenetVisualizerStyle,
    label: &str,
    text_format: TextFormat,
    top_value: TopValue,
    values: Vec<f32>,
) {
    if values.is_empty() {
        return;
    }

    ui.vertical(|ui| {
        ui.label(RichText::new(label).heading().color(style.text_color));

        let last_value = values.last().unwrap();

        let min = 0.0;
        let mut max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        match top_value {
            TopValue::MaxValue { multiplicated } => {
                max *= multiplicated;
            }
            TopValue::SuggestedValues(suggested_values) => {
                for value in suggested_values.into_iter() {
                    if max < value {
                        max = value;
                        break;
                    }
                }
            }
        }

        let spacing_x = ui.spacing().item_spacing.x;

        let last_text: WidgetText = match text_format {
            TextFormat::Normal => format!("{:.2}", last_value).into(),
            TextFormat::Percentage => format!("{:.1}%", last_value * 100.).into(),
        };
        let galley = last_text.into_galley(ui, Some(false), f32::INFINITY, TextStyle::Button);
        let (outer_rect, _) = ui.allocate_exact_size(Vec2::new(style.width + galley.size().x + spacing_x, style.height), Sense::hover());
        let rect = Rect::from_min_size(outer_rect.left_top(), vec2(style.width, style.height));
        let text_pos = rect.right_center() + vec2(spacing_x / 2.0, -galley.size().y / 2.);
        galley.paint_with_fallback_color(&ui.painter().with_clip_rect(outer_rect), text_pos, style.text_color);

        let body = Shape::Rect(RectShape {
            rect,
            rounding: Rounding::none(),
            fill: Rgba::TRANSPARENT.into(),
            stroke: style.rectangle_stroke,
        });
        ui.painter().add(body);
        let init_point = rect.left_bottom();

        let size = values.len();
        let points = values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let x = remap(i as f32, 0.0..=size as f32, 0.0..=style.width);
                let y = remap(*value, min..=max, 0.0..=style.height);

                pos2(x + init_point.x, init_point.y - y)
            })
            .collect();

        let path = PathShape::line(points, style.line_stroke);
        ui.painter().add(path);

        {
            let text: WidgetText = match text_format {
                TextFormat::Normal => format!("{:.0}", max).into(),
                TextFormat::Percentage => format!("{:.0}%", max * 100.).into(),
            };
            let galley = text.into_galley(ui, Some(false), f32::INFINITY, TextStyle::Button);
            let text_pos = rect.left_top() + Vec2::new(0.0, galley.size().y / 2.) + vec2(spacing_x, 0.0);
            galley.paint_with_fallback_color(&ui.painter().with_clip_rect(rect), text_pos, style.text_color);
        }
        {
            let text: WidgetText = match text_format {
                TextFormat::Normal => format!("{:.0}", min).into(),
                TextFormat::Percentage => format!("{:.0}%", min * 100.).into(),
            };
            let galley = text.into_galley(ui, Some(false), f32::INFINITY, TextStyle::Button);
            let text_pos = rect.left_bottom() - Vec2::new(0.0, galley.size().y * 1.5) + vec2(spacing_x, 0.0);
            galley.paint_with_fallback_color(&ui.painter().with_clip_rect(rect), text_pos, style.text_color);
        }
    });
}
