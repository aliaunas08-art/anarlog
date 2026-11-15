import AppKit
import SwiftUI

enum FloatingBarLayout {
  static let inset: CGFloat = 4
  static let screenMargin: CGFloat = 8
  static let compactHeight: CGFloat = 38
  static let compactCornerRadius: CGFloat = 19
  static let controlCornerRadius: CGFloat = 15
  // The status segment holds the live dot, timer, level meter and Stop.
  // Keep in sync with layout.ts and the Rust layout module.
  static let compactStopWidth: CGFloat = 176
  static let compactSoloStopWidth: CGFloat = 180
  static let compactIconSize: CGFloat = 30
  static let compactGap: CGFloat = 0
  static let compactHorizontalPadding: CGFloat = 4
  static let expandedWidth: CGFloat = 360
  static let expandedHeight: CGFloat = 430
  static let expandedCornerRadius: CGFloat = 21
  static let expandedPadding: CGFloat = 12
  static let liveDotSize: CGFloat = 8
  static let stopButtonHeight: CGFloat = 24
  static let meterBars = 10
  static let meterHeight: CGFloat = 16
  static let hoverHandleGap: CGFloat = 2
  static let hoverHandleTopPadding: CGFloat = 7
  static let hoverHandleHeight: CGFloat = 12
  static let hoverHandleReservedHeight: CGFloat =
    hoverHandleTopPadding + hoverHandleHeight + hoverHandleGap
  static let hoverHandleDotSize: CGFloat = 1.6
  static let hoverHandleDotColumnSpacing: CGFloat = 5
  static let hoverHandleDotRowSpacing: CGFloat = 7
  static let hoverHandleHorizontalPadding: CGFloat = 8
  static let dragClickThreshold: CGFloat = 4

  static func compactControlsWidth(showsExpand: Bool) -> CGFloat {
    showsExpand ? compactStopWidth + compactGap + compactIconSize : compactSoloStopWidth
  }

  static func compactWidth(showsExpand: Bool) -> CGFloat {
    compactControlsWidth(showsExpand: showsExpand)
      + compactHorizontalPadding * 2
  }

  static func containerSize(isExpanded: Bool, showsExpand: Bool)
    -> NSSize
  {
    NSSize(
      width: (isExpanded
        ? expandedWidth : compactWidth(showsExpand: showsExpand)) + inset
        * 2,
      height: (isExpanded ? expandedHeight : compactHeight) + hoverHandleReservedHeight + inset * 2)
  }

}

struct FloatingBarView: View {
  @ObservedObject var model: FloatingBarViewModel
  @ObservedObject var settings: FloatingOverlaySettingsModel
  let panelOrigin: () -> NSPoint?
  let movePanel: (NSPoint) -> Void
  @State private var isBarHovered = false
  @State private var shouldAutoScrollTranscript = true
  @State private var suppressNextClick = false
  @State private var dragStart: FloatingBarDragStart?
  private let transcriptBottomAnchorId = "floating-transcript-bottom-anchor"
  private let transcriptNoticeId = "floating-transcript-notice"

  private var bodyHeight: CGFloat {
    containerSize.height - FloatingBarLayout.inset * 2 - FloatingBarLayout.hoverHandleReservedHeight
  }

  private var expandsUpward: Bool { model.placement?.expandsUpward ?? true }

  var body: some View {
    let showsHoverHandle = isBarHovered && !model.isExpanded
    let width = containerSize.width - FloatingBarLayout.inset * 2
    let radius =
      model.isExpanded
      ? FloatingBarLayout.expandedCornerRadius : FloatingBarLayout.compactCornerRadius
    let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
    ZStack(alignment: .bottom) {
      if showsHoverHandle {
        FloatingBarHoverHandle(color: dragHandleDotColor, width: width)
          .frame(height: FloatingBarLayout.hoverHandleHeight)
          .padding(.top, FloatingBarLayout.hoverHandleTopPadding)
          .frame(maxHeight: .infinity, alignment: .top)
          .accessibilityHidden(true)
      }
      ZStack(alignment: .topLeading) {
        if model.isExpanded {
          expandedPanel
            .frame(width: width, height: max(0, bodyHeight - FloatingBarLayout.compactHeight))
            .offset(y: expandsUpward ? 0 : FloatingBarLayout.compactHeight)
        }
        floatingControls(isExpanded: model.isExpanded)
          .frame(
            width: FloatingBarLayout.compactControlsWidth(
              showsExpand: model.liveCaptionToggleVisible),
            height: FloatingBarLayout.compactHeight
          )
          .position(
            x: (model.placement?.controlsCenterX ?? containerSize.width / 2)
              - FloatingBarLayout.inset,
            y: expandsUpward
              ? bodyHeight - FloatingBarLayout.compactHeight / 2
              : FloatingBarLayout.compactHeight / 2)
      }
      .frame(width: width, height: bodyHeight, alignment: .topLeading)
    }
    .frame(
      width: width,
      height: bodyHeight + (showsHoverHandle ? FloatingBarLayout.hoverHandleReservedHeight : 0),
      alignment: .bottom
    )
    .background(shape.fill(isBarHovered && !model.isExpanded ? envelopeSurfaceColor : surfaceColor))
    .overlay(shape.strokeBorder(hairlineColor, lineWidth: 0.5))
    .clipShape(shape)
    // Soft shadow that fits inside the transparent window inset.
    .compositingGroup()
    .shadow(color: shadowColor, radius: 1.5, y: 1)
    .padding(FloatingBarLayout.inset)
    .frame(width: containerSize.width, height: containerSize.height, alignment: .bottom)
    .contentShape(Rectangle())
    .simultaneousGesture(dragClickSuppressor)
    .onHover { isBarHovered = $0 }
    .animation(.easeOut(duration: 0.12), value: isBarHovered)
  }

  private var expandedPanel: some View {
    VStack(spacing: 12) {
      ScrollViewReader { proxy in
        ZStack(alignment: .bottom) {
          ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 8) {
              if let dictation = model.dictation {
                DictationTranscript(dictation: dictation, color: labelColor)
              } else {
                ForEach(Array(model.transcriptBubbles.enumerated()), id: \.element.id) {
                  index, bubble in
                  TranscriptBubbleView(
                    bubble: bubble,
                    showsSpeakerLabel: showsSpeakerLabel(at: index),
                    palette: palette
                  )
                  .id(bubble.id)
                }
                if let notice = model.transcriptNotice {
                  Text(notice)
                    .font(.system(size: 12))
                    .foregroundStyle(secondaryLabelColor)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)
                    .id(transcriptNoticeId)
                }
              }
              Color.clear
                .frame(height: FloatingBarLayout.expandedPadding)
                .id(transcriptBottomAnchorId)
            }
            .frame(maxWidth: .infinity, alignment: .bottom)
            .background(
              TranscriptScrollObserver(isPinnedToBottom: $shouldAutoScrollTranscript)
            )
          }
          .frame(maxWidth: .infinity, maxHeight: .infinity)
          .onChange(of: model.transcriptBubbles.last?.id) { _, bubbleId in
            if bubbleId != nil, shouldAutoScrollTranscript {
              scrollTranscriptToBottom(proxy)
            }
          }
          .onChange(of: model.transcriptNotice) { _, notice in
            if notice != nil, shouldAutoScrollTranscript {
              scrollTranscriptToBottom(proxy)
            }
          }
          .onChange(of: model.dictation) { _, _ in
            if shouldAutoScrollTranscript { scrollTranscriptToBottom(proxy) }
          }
          .onAppear {
            shouldAutoScrollTranscript = true
            scrollTranscriptToBottom(proxy)
          }

          if !shouldAutoScrollTranscript,
            model.dictation != nil || model.transcriptBubbles.last?.id != nil
          {
            transcriptBottomChip {
              performClick {
                scrollTranscriptToBottom(proxy, animated: true)
                shouldAutoScrollTranscript = true
              }
            }
            .padding(.bottom, 0)
            .transition(.move(edge: .bottom))
          }
        }
        .animation(.easeOut(duration: 0.12), value: shouldAutoScrollTranscript)
      }
      .id(model.dictation?.sessionId)
      .padding(.horizontal, FloatingBarLayout.expandedPadding)
      .padding(.vertical, FloatingBarLayout.expandedPadding)
    }
  }

  private func floatingControls(isExpanded: Bool) -> some View {
    HStack(spacing: FloatingBarLayout.compactGap) {
      audioControl(
        width: model.liveCaptionToggleVisible
          ? FloatingBarLayout.compactStopWidth : FloatingBarLayout.compactSoloStopWidth,
        height: FloatingBarLayout.compactIconSize
      )

      if model.liveCaptionToggleVisible {
        FloatingIconButton(
          systemName: isExpanded
            ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right",
          accessibilityLabel: isExpanded ? "Collapse live transcript" : "Expand live transcript",
          color: secondaryLabelColor,
          hoverFill: controlHoverFill,
          size: FloatingBarLayout.compactIconSize,
          action: { performClick { setExpanded(!isExpanded) } }
        )
      }
    }
  }

  /// Live dot · timer · level meter · Stop (or Done while dictating).
  private func audioControl(width: CGFloat, height: CGFloat) -> some View {
    let finishing = model.dictation?.phase == "transcribing"
    let notice = model.transcriptNotice ?? "Recording needs attention"

    return HStack(spacing: 8) {
      if model.status == .error {
        Image(systemName: "exclamationmark.circle")
          .font(.system(size: 12, weight: .semibold))
          .foregroundStyle(labelColor)
          .help("\(notice). Open Anarlog to retry or change devices.")
          .accessibilityLabel(notice)
      } else {
        FloatingLiveDot()
          .accessibilityHidden(true)
      }

      if model.dictation == nil, let elapsed = model.elapsedSeconds {
        let text = FloatingBarTimer.format(elapsed)
        Text(text)
          .font(.system(size: 13, weight: .semibold).monospacedDigit())
          .foregroundStyle(labelColor)
          .lineLimit(1)
          .fixedSize()
          .accessibilityLabel("Recording time \(text)")
      }

      Group {
        if finishing {
          Text("Finishing…")
            .font(.system(size: 12))
            .foregroundStyle(secondaryLabelColor)
            .lineLimit(1)
        } else {
          FloatingLevelMeter(amplitude: model.amplitude, color: labelColor)
            .opacity(model.status == .reconnecting ? 0.4 : 1)
            .accessibilityHidden(true)
        }
      }
      .frame(maxWidth: .infinity, alignment: .leading)
      .clipped()

      Button(action: {
        performClick {
          if model.dictation != nil {
            dictationAction("finish")
          } else {
            RustBridge.stopListening()
          }
        }
      }) {
        Text(model.dictation == nil ? "Stop" : "Done")
          .font(.system(size: 12, weight: .medium))
          .foregroundStyle(windowColor)
          .padding(.horizontal, 10)
          .frame(height: FloatingBarLayout.stopButtonHeight)
          .background(Capsule(style: .continuous).fill(labelColor))
          .contentShape(Capsule(style: .continuous))
      }
      .buttonStyle(FloatingStopButtonStyle())
      .disabled(finishing)
      .accessibilityLabel(
        model.dictation != nil
          ? "Finish dictation"
          : model.status == .reconnecting
            ? "Reconnecting live transcription; stop listening"
            : model.status == .error
              ? "\(notice); stop listening"
              : "Stop listening"
      )
      .help(model.dictation == nil ? "Stop listening" : "Finish dictation")
    }
    .padding(.leading, 10)
    .padding(.trailing, 3)
    .frame(width: width, height: height)
  }

  private var containerSize: NSSize {
    model.placement?.frame.size
      ?? FloatingBarLayout.containerSize(
        isExpanded: model.isExpanded,
        showsExpand: model.liveCaptionToggleVisible
      )
  }

  private var palette: FloatingBarPalette {
    FloatingBarPalette(colorScheme: model.colorScheme)
  }

  /// Neutral pill (`--cf-glass-solid` / white). The opacity setting still
  /// applies but never drops the surface below legible.
  private var surfaceColor: Color {
    let opacity = min(max(settings.floatingBarOpacity, 0.35), 0.95)
    return surfaceSolidColor.opacity(0.55 + (opacity - 0.35) / 0.6 * 0.45)
  }

  private var envelopeSurfaceColor: Color { surfaceSolidColor }
  private var surfaceSolidColor: Color { palette.surface }
  private var labelColor: Color { palette.label }
  private var secondaryLabelColor: Color { palette.secondary }
  private var windowColor: Color { palette.window }
  private var hairlineColor: Color { palette.hairline }
  private var shadowColor: Color { palette.shadow }
  private var controlHoverFill: Color { palette.hover }
  private var dragHandleDotColor: Color { palette.handle }

  private var dragClickSuppressor: some Gesture {
    DragGesture(
      minimumDistance: FloatingBarLayout.dragClickThreshold,
      coordinateSpace: .global
    )
    .onChanged { _ in
      suppressNextClick = true

      let mouseLocation = NSEvent.mouseLocation
      let start =
        dragStart
        ?? panelOrigin().map {
          FloatingBarDragStart(panelOrigin: $0, mouseLocation: mouseLocation)
        }

      guard let start else { return }
      dragStart = start

      movePanel(
        NSPoint(
          x: start.panelOrigin.x + mouseLocation.x - start.mouseLocation.x,
          y: start.panelOrigin.y + mouseLocation.y - start.mouseLocation.y
        )
      )
    }
    .onEnded { _ in
      dragStart = nil
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
        suppressNextClick = false
      }
    }
  }

  private func performClick(_ action: () -> Void) {
    if suppressNextClick {
      suppressNextClick = false
      return
    }

    action()
  }

  private func setExpanded(_ expanded: Bool) {
    if model.dictation != nil {
      dictationAction("togglePreview")
    } else {
      model.isExpanded = expanded
      settings.setLiveCaptionMinimized(!expanded)
    }
  }

  private func dictationAction(_ action: String) {
    guard let dictation = model.dictation else { return }
    let payload = ["sessionId": dictation.sessionId, "action": action]
    if let data = try? JSONSerialization.data(withJSONObject: payload),
      let json = String(data: data, encoding: .utf8)
    {
      json.withCString { rust_on_floating_bar_dictation_action($0) }
    }
  }

  private func showsSpeakerLabel(at index: Int) -> Bool {
    guard model.transcriptBubbles.indices.contains(index) else { return false }
    guard index > model.transcriptBubbles.startIndex else { return true }

    let bubble = model.transcriptBubbles[index]
    let previousBubble = model.transcriptBubbles[index - 1]
    return bubble.speakerLabel != previousBubble.speakerLabel
      || bubble.isSelf != previousBubble.isSelf
  }

  private func scrollTranscriptToBottom(_ proxy: ScrollViewProxy, animated: Bool = false) {
    DispatchQueue.main.async {
      if animated {
        withAnimation(.easeOut(duration: 0.16)) {
          proxy.scrollTo(transcriptBottomAnchorId, anchor: .bottom)
        }
      } else {
        proxy.scrollTo(transcriptBottomAnchorId, anchor: .bottom)
      }
    }
  }

  private func transcriptBottomChip(action: @escaping () -> Void) -> some View {
    let shape = Capsule(style: .continuous)

    return Button(action: action) {
      HStack(spacing: 6) {
        Image(systemName: "arrow.down")
          .font(.system(size: 10, weight: .semibold))
        Text("Go to bottom")
          .font(.system(size: 11, weight: .medium))
      }
      .foregroundStyle(labelColor)
      .padding(.horizontal, 12)
      .padding(.vertical, 7)
      .background(
        shape
          .fill(transcriptChipFillColor)
      )
      .overlay(
        shape
          .strokeBorder(transcriptChipStrokeColor, lineWidth: 0.5)
      )
    }
    .buttonStyle(.plain)
    .accessibilityLabel("Scroll transcript to bottom")
  }

  private var transcriptChipFillColor: Color { palette.surface }

  private var transcriptChipStrokeColor: Color { palette.hairlineStrong }
}

private struct FloatingBarDragStart {
  let panelOrigin: NSPoint
  let mouseLocation: NSPoint
}

private struct FloatingBarHoverHandle: View {
  let color: Color
  let width: CGFloat

  var body: some View {
    FloatingBarDotPattern(color: color)
      .frame(
        width: max(0, width - FloatingBarLayout.hoverHandleHorizontalPadding * 2),
        height: FloatingBarLayout.hoverHandleHeight
      )
      .padding(.horizontal, FloatingBarLayout.hoverHandleHorizontalPadding)
  }
}

private struct FloatingBarDotPattern: View {
  let color: Color

  var body: some View {
    Canvas { context, size in
      var y = FloatingBarLayout.hoverHandleDotSize / 2
      while y <= size.height {
        var x = FloatingBarLayout.hoverHandleDotSize / 2
        while x <= size.width {
          let rect = CGRect(
            x: x - FloatingBarLayout.hoverHandleDotSize / 2,
            y: y - FloatingBarLayout.hoverHandleDotSize / 2,
            width: FloatingBarLayout.hoverHandleDotSize,
            height: FloatingBarLayout.hoverHandleDotSize
          )
          context.fill(Path(ellipseIn: rect), with: .color(color))
          x += FloatingBarLayout.hoverHandleDotColumnSpacing
        }
        y += FloatingBarLayout.hoverHandleDotRowSpacing
      }
    }
  }
}

private struct TranscriptScrollObserver: NSViewRepresentable {
  @Binding var isPinnedToBottom: Bool

  func makeCoordinator() -> Coordinator {
    Coordinator()
  }

  func makeNSView(context: Context) -> NSView {
    let view = NSView()
    DispatchQueue.main.async {
      context.coordinator.bind(to: view.enclosingScrollView)
    }
    return view
  }

  func updateNSView(_ view: NSView, context: Context) {
    context.coordinator.isPinnedToBottom = $isPinnedToBottom
    DispatchQueue.main.async {
      context.coordinator.bind(to: view.enclosingScrollView)
    }
  }

  final class Coordinator {
    var isPinnedToBottom: Binding<Bool>?
    private weak var scrollView: NSScrollView?
    private var boundsObserver: NSObjectProtocol?
    private let threshold: CGFloat = 20

    deinit {
      if let boundsObserver {
        NotificationCenter.default.removeObserver(boundsObserver)
      }
    }

    func bind(to scrollView: NSScrollView?) {
      guard self.scrollView !== scrollView else { return }

      if let boundsObserver {
        NotificationCenter.default.removeObserver(boundsObserver)
      }

      self.scrollView = scrollView
      guard let scrollView else { return }

      scrollView.contentView.postsBoundsChangedNotifications = true
      boundsObserver = NotificationCenter.default.addObserver(
        forName: NSView.boundsDidChangeNotification,
        object: scrollView.contentView,
        queue: .main
      ) { [weak self] _ in
        self?.updatePinnedState()
      }

      updatePinnedState()
    }

    func updatePinnedState() {
      guard let scrollView, let documentView = scrollView.documentView else { return }

      let visibleRect = scrollView.documentVisibleRect
      let documentBounds = documentView.bounds
      let isPinned: Bool
      if documentView.isFlipped {
        isPinned = visibleRect.maxY >= documentBounds.maxY - threshold
      } else {
        isPinned = visibleRect.minY <= documentBounds.minY + threshold
      }

      if isPinnedToBottom?.wrappedValue != isPinned {
        isPinnedToBottom?.wrappedValue = isPinned
      }
    }
  }
}

private struct FloatingIconButton: View {
  let systemName: String
  let accessibilityLabel: String
  let color: Color
  let hoverFill: Color
  let size: CGFloat
  let action: () -> Void
  @State private var isHovered = false

  var body: some View {
    let shape = RoundedRectangle(
      cornerRadius: FloatingBarLayout.controlCornerRadius,
      style: .continuous
    )

    return Button(action: action) {
      Image(systemName: systemName)
        .font(.system(size: 12, weight: .semibold))
        .foregroundStyle(color)
        .frame(width: size, height: size)
        .background(
          shape
            .fill(isHovered ? hoverFill : Color.clear)
        )
        .contentShape(shape)
    }
    .buttonStyle(.plain)
    .accessibilityLabel(accessibilityLabel)
    .onHover { isHovered = $0 }
  }
}

private struct TranscriptBubbleView: View {
  let bubble: FloatingTranscriptBubblePayload
  let showsSpeakerLabel: Bool
  let palette: FloatingBarPalette

  var body: some View {
    HStack {
      if bubble.isSelf {
        Spacer(minLength: 40)
      }

      VStack(alignment: .leading, spacing: 4) {
        if showsSpeakerLabel || isOverlapping {
          HStack(spacing: 4) {
            speakerLabel
            overlapGlyph
          }
          .frame(maxWidth: .infinity, alignment: .leading)
        }

        let shape = RoundedRectangle(cornerRadius: 11, style: .continuous)
        Text(bubble.text)
          .font(.system(size: 13, weight: .regular))
          .foregroundStyle(palette.label)
          .multilineTextAlignment(.leading)
          .frame(maxWidth: .infinity, alignment: .leading)
          .fixedSize(horizontal: false, vertical: true)
          .padding(.horizontal, 11)
          .padding(.vertical, 8)
          .background(shape.fill(bubble.isSelf ? palette.bubbleSelf : palette.bubble))
          .overlay(
            shape
              .strokeBorder(palette.hairlineStrong, lineWidth: isOverlapping ? 1 : 0)
          )
      }

      if !bubble.isSelf {
        Spacer(minLength: 40)
      }
    }
  }

  private var speakerLabel: some View {
    Group {
      if showsSpeakerLabel {
        Text(bubble.speakerLabel)
          .font(.system(size: 10, weight: .semibold))
          .foregroundStyle(palette.secondary)
          .lineLimit(1)
      }
    }
  }

  private var overlapGlyph: some View {
    Group {
      if isOverlapping {
        Image(systemName: "arrow.left.and.right")
          .font(.system(size: 8, weight: .bold))
          .foregroundStyle(palette.secondary)
          .frame(width: 12, height: 12)
          .accessibilityLabel("Overlapping speech")
      }
    }
  }

  private var isOverlapping: Bool {
    bubble.overlapsPrevious || bubble.overlapsNext
  }
}

/// Neutral palette mirroring the app's `--cf-*` tokens (c-final.css).
struct FloatingBarPalette {
  let colorScheme: FloatingBarColorScheme

  private var dark: Bool { colorScheme == .dark }

  /// `--cf-glass-solid` in dark (#1c1c1e), white in light.
  var surface: Color { dark ? rgb(28, 28, 30) : .white }
  var label: Color { dark ? rgb(245, 245, 247) : rgb(29, 29, 31) }
  var secondary: Color { dark ? rgb(161, 161, 166) : rgb(110, 110, 115) }
  var window: Color { dark ? .black : rgb(251, 251, 253) }
  var hairline: Color { dark ? Color.white.opacity(0.1) : Color.black.opacity(0.08) }
  var hairlineStrong: Color { dark ? Color.white.opacity(0.24) : Color.black.opacity(0.2) }
  var shadow: Color { Color.black.opacity(dark ? 0.45 : 0.16) }
  var hover: Color { dark ? Color.white.opacity(0.08) : Color.black.opacity(0.05) }
  var handle: Color { dark ? Color.white.opacity(0.35) : Color.black.opacity(0.25) }
  var bubble: Color { dark ? Color.white.opacity(0.08) : Color.black.opacity(0.05) }
  var bubbleSelf: Color { dark ? Color.white.opacity(0.14) : Color.black.opacity(0.09) }

  private func rgb(_ red: Double, _ green: Double, _ blue: Double) -> Color {
    Color(red: red / 255, green: green / 255, blue: blue / 255)
  }
}

enum FloatingBarTimer {
  /// m:ss, or h:mm:ss past an hour (matches `formatElapsed` in the app).
  static func format(_ totalSeconds: Double) -> String {
    let safe = max(0, Int(totalSeconds.rounded(.down)))
    let hours = safe / 3600
    let minutes = (safe % 3600) / 60
    let seconds = safe % 60
    return hours > 0
      ? String(format: "%d:%02d:%02d", hours, minutes, seconds)
      : String(format: "%d:%02d", minutes, seconds)
  }
}

/// `--cf-record`: the bar's only red. Pulses like `.cf-record-dot`; still with
/// reduced motion.
private struct FloatingLiveDot: View {
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var pulsing = false

  private static let red = Color(red: 1, green: 59 / 255, blue: 48 / 255)

  var body: some View {
    Circle()
      .fill(Self.red)
      .frame(width: FloatingBarLayout.liveDotSize, height: FloatingBarLayout.liveDotSize)
      .background(
        Circle()
          .fill(Self.red.opacity(reduceMotion ? 0 : (pulsing ? 0 : 0.35)))
          .scaleEffect(reduceMotion ? 1 : (pulsing ? 2.25 : 1))
      )
      .onAppear { updatePulse() }
      .onChange(of: reduceMotion) { _, _ in updatePulse() }
  }

  private func updatePulse() {
    guard !reduceMotion else {
      pulsing = false
      return
    }
    pulsing = false
    withAnimation(.easeOut(duration: 1.6).repeatForever(autoreverses: false)) {
      pulsing = true
    }
  }
}

/// The in-app pill's level meter (src/shared/level-meter.tsx): a rolling row
/// of thin bars, newest on the right. Still with reduced motion.
struct FloatingLevelMeter: View {
  let amplitude: Double
  let color: Color
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var history = Array(repeating: 0.0, count: FloatingBarLayout.meterBars)

  private static let sampleInterval: TimeInterval = 0.12
  private static let staticLevel = 0.3
  // Shared so re-renders (amplitude arrives many times a second) do not
  // restart the sampling timer.
  private static let ticker = Timer.publish(every: sampleInterval, on: .main, in: .common)
    .autoconnect()

  var body: some View {
    HStack(spacing: 2) {
      ForEach(Array(values.enumerated()), id: \.offset) { _, value in
        RoundedRectangle(cornerRadius: 1, style: .continuous)
          .fill(color)
          .opacity(0.35 + value * 0.65)
          .frame(
            width: 2,
            height: max(3, (value * FloatingBarLayout.meterHeight).rounded()))
      }
    }
    .frame(height: FloatingBarLayout.meterHeight)
    .onReceive(Self.ticker) { _ in
      guard !reduceMotion else { return }
      history = Array(history.dropFirst()) + [min(max(amplitude, 0), 1)]
    }
  }

  private var values: [Double] {
    reduceMotion ? history.map { _ in Self.staticLevel } : history
  }
}

private struct FloatingStopButtonStyle: ButtonStyle {
  @Environment(\.isEnabled) private var isEnabled

  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .opacity(!isEnabled ? 0.6 : configuration.isPressed ? 0.85 : 1)
  }
}
