import AppKit
@preconcurrency import SwiftUI

typealias QuickGUIActionCallback = @convention(c) (
  UnsafeMutableRawPointer?,
  UInt64
) -> Void

typealias QuickGUIPresentationCallback = @convention(c) (
  UnsafeMutableRawPointer?,
  UInt64,
  Bool
) -> Void

typealias QuickGUIValueCallback = @convention(c) (
  UnsafeMutableRawPointer?,
  UInt64,
  UnsafePointer<CChar>?
) -> Void

typealias QuickGUISubmitCallback = @convention(c) (
  UnsafeMutableRawPointer?,
  UInt64
) -> Void

private let quickGUIGlassEffectInset: CGFloat = 24
/// Headroom every non-empty host keeps around ordinary controls: a bezel's shadow and the focus
/// ring are drawn a few points outside the control's own bounds and would otherwise be clipped at
/// its layout box.
private let quickGUIControlEffectInset: CGFloat = 6

private struct QuickGUIPickerOption: Decodable, Equatable {
  let value: String
  let label: String
  let systemImage: String?
  let disabled: Bool
}

private struct QuickGUIElement: Decodable, Identifiable, Equatable {
  let id: UInt64
  let type: String
  let label: String?
  let systemImage: String?
  let role: String?
  let target: String?
  let testID: String?
  let modifiers: [QuickGUIModifier]?
  let hasAction: Bool?
  let value: Double?
  let minimum: Double?
  let maximum: Double?
  let step: Double?
  let isOn: Bool?
  let total: Double?
  let currentValueLabel: String?
  let text: String?
  let placeholder: String?
  let secure: Bool?
  let hasValueChange: Bool?
  let hasSubmit: Bool?
  let selection: String?
  let options: [QuickGUIPickerOption]?
  let style: String?
  let components: String?
  let supportsOpacity: Bool?
  let minimumValueLabel: String?
  let maximumValueLabel: String?
  let matchHorizontal: Bool?
  let matchVertical: Bool?
  let width: Double?
  let height: Double?
  let isPresented: Bool?
  let attachmentAnchor: String?
  let arrowEdge: String?
  let trigger: [QuickGUIElement]?
  let content: [QuickGUIElement]?
}

private struct QuickGUIModifier: Decodable, Equatable {
  let type: String
  let style: String?
  let size: String?
  let shape: String?
  let cornerRadius: Double?
  let color: String?
  let disabled: Bool?

  private enum CodingKeys: String, CodingKey {
    case type = "$type"
    case style
    case size
    case shape
    case cornerRadius
    case color
    case disabled
  }
}

private final class QuickGUIActionSink {
  let context: UnsafeMutableRawPointer?
  let actionCallback: QuickGUIActionCallback?
  let presentationCallback: QuickGUIPresentationCallback?
  let valueCallback: QuickGUIValueCallback?
  let submitCallback: QuickGUISubmitCallback?

  init(
    context: UnsafeMutableRawPointer?,
    actionCallback: QuickGUIActionCallback?,
    presentationCallback: QuickGUIPresentationCallback?,
    valueCallback: QuickGUIValueCallback?,
    submitCallback: QuickGUISubmitCallback?
  ) {
    self.context = context
    self.actionCallback = actionCallback
    self.presentationCallback = presentationCallback
    self.valueCallback = valueCallback
    self.submitCallback = submitCallback
  }

  func sendAction(_ id: UInt64) {
    actionCallback?(context, id)
  }

  func sendPresentation(_ id: UInt64, _ isPresented: Bool) {
    presentationCallback?(context, id, isPresented)
  }

  func sendValue(_ id: UInt64, _ value: String) {
    guard let valueCallback else { return }
    value.withCString { pointer in
      valueCallback(context, id, pointer)
    }
  }

  func sendSubmit(_ id: UInt64) {
    submitCallback?(context, id)
  }
}

private struct QuickGUIEmbeddedEntry {
  let view: NSView
  let size: NSSize
}

/// Padding the host keeps around its content so controls can draw past their own bounds without
/// being clipped. Ordinary controls need a few points for bezels and focus rings; Liquid Glass
/// needs more; an empty host needs none.
private func quickGUIEffectInset(for elements: [QuickGUIElement]) -> CGFloat {
  if elements.isEmpty {
    return 0
  }
  if elements.contains(where: quickGUIReservesGlassInset) {
    return quickGUIGlassEffectInset
  }
  return quickGUIControlEffectInset
}

/// Whether an element draws a Liquid Glass effect that extends well past its own bounds.
///
/// Popover content is presented in its own window and never counts.
private func quickGUIReservesGlassInset(_ element: QuickGUIElement) -> Bool {
  guard #available(macOS 26.0, *) else { return false }
  if element.type == "picker", element.style == "tabs" {
    return true
  }
  let glassButton = (element.modifiers ?? []).contains { modifier in
    modifier.type == "buttonStyle"
      && (modifier.style == "glass" || modifier.style == "glassProminent")
  }
  if glassButton {
    return true
  }
  return (element.trigger ?? []).contains(where: quickGUIReservesGlassInset)
}

private final class QuickGUIElementStore: ObservableObject {
  @Published var elements: [QuickGUIElement] = []
  @Published private(set) var embeddedRevision: UInt64 = 0
  /// Padding the root view keeps around the hosted content so effects drawn past a control's
  /// bounds, from a focus ring to Liquid Glass, are not clipped by the hosting view. The Rust side
  /// reports it as the view's outset so it never enters QuickGUI layout.
  @Published private(set) var effectInset: CGFloat = 0
  private var embedded: [UInt64: QuickGUIEmbeddedEntry] = [:]

  func updateElements(_ elements: [QuickGUIElement]) {
    self.elements = elements
    let inset = quickGUIEffectInset(for: elements)
    if inset != effectInset {
      effectInset = inset
    }
  }

  func setEmbeddedView(_ id: UInt64, view: NSView, size: NSSize) {
    let normalized = NSSize(width: max(1, size.width), height: max(1, size.height))
    if let previous = embedded[id], previous.view === view, previous.size == normalized {
      return
    }
    embedded[id] = QuickGUIEmbeddedEntry(view: view, size: normalized)
    embeddedRevision &+= 1
  }

  func removeEmbeddedView(_ id: UInt64) {
    guard embedded.removeValue(forKey: id) != nil else { return }
    embeddedRevision &+= 1
  }

  func embeddedView(_ id: UInt64) -> QuickGUIEmbeddedEntry? {
    embedded[id]
  }

}

private final class QuickGUIEmbeddedContainerView: NSView {
  private var hostedView: NSView?
  private var contentSize = NSSize(width: 1, height: 1)

  override var isFlipped: Bool { true }

  override var intrinsicContentSize: NSSize {
    contentSize
  }

  func update(entry: QuickGUIEmbeddedEntry?) {
    let nextView = entry?.view
    if hostedView !== nextView {
      hostedView?.removeFromSuperview()
      hostedView = nextView
      if let nextView {
        nextView.autoresizingMask = [.width, .height]
        addSubview(nextView)
      }
    }
    if let entry, contentSize != entry.size {
      contentSize = entry.size
      invalidateIntrinsicContentSize()
    }
    needsLayout = true
  }

  override func layout() {
    super.layout()
    hostedView?.frame = bounds
  }

  func detach() {
    hostedView?.removeFromSuperview()
    hostedView = nil
  }
}

private struct QuickGUIEmbeddedRepresentable: NSViewRepresentable {
  @ObservedObject var store: QuickGUIElementStore
  let id: UInt64

  func makeNSView(context: Context) -> QuickGUIEmbeddedContainerView {
    let view = QuickGUIEmbeddedContainerView(frame: .zero)
    view.setAccessibilityElement(false)
    view.update(entry: store.embeddedView(id))
    return view
  }

  func updateNSView(_ nsView: QuickGUIEmbeddedContainerView, context: Context) {
    _ = store.embeddedRevision
    nsView.update(entry: store.embeddedView(id))
  }

  static func dismantleNSView(_ nsView: QuickGUIEmbeddedContainerView, coordinator: ()) {
    nsView.detach()
  }
}

/// Compatibility bridge for the macOS 27 segmented-tabs role. Xcode 27 exposes this as
/// SwiftUI's `.tabs` picker style; when QuickGUI is built by Xcode 26, the public AppKit role is
/// selected dynamically so an app running on macOS 27 still receives the native treatment.
private struct QuickGUITabsPickerRepresentable: NSViewRepresentable {
  let element: QuickGUIElement
  let actionSink: QuickGUIActionSink

  func makeCoordinator() -> Coordinator {
    Coordinator(actionSink: actionSink)
  }

  func makeNSView(context: Context) -> NSSegmentedControl {
    let control = NSSegmentedControl(frame: .zero)
    control.trackingMode = .selectOne
    control.segmentDistribution = .fillEqually
    control.target = context.coordinator
    control.action = #selector(Coordinator.selectionChanged(_:))
    return control
  }

  func updateNSView(_ control: NSSegmentedControl, context: Context) {
    let options = element.options ?? []
    context.coordinator.update(
      id: element.id,
      options: options,
      hasValueChange: element.hasValueChange ?? false
    )

    control.segmentCount = options.count
    control.isEnabled = context.environment.isEnabled
    control.controlSize = quickGUIAppKitControlSize(context.environment.controlSize)

    let roleSelector = NSSelectorFromString("setRole:")
    if control.responds(to: roleSelector) {
      // NSSegmentedControl.Role.tabs. KVC keeps this bridge buildable with the Xcode 26 SDK while
      // still selecting the public macOS 27 role at runtime.
      control.setValue(NSNumber(value: 1), forKey: "role")
      control.segmentStyle = .automatic
    } else {
      control.segmentStyle = .capsule
    }
    if #available(macOS 26.0, *) {
      control.borderShape = .capsule
    }

    var selectedSegment = -1
    for (index, option) in options.enumerated() {
      control.setImage(nil, forSegment: index)
      control.setLabel(option.label, forSegment: index)
      if option.label.isEmpty, let name = option.systemImage,
        let image = NSImage(systemSymbolName: name, accessibilityDescription: option.value)
      {
        control.setImage(image, forSegment: index)
      }
      control.setEnabled(context.environment.isEnabled && !option.disabled, forSegment: index)
      control.setToolTip(option.label.isEmpty ? nil : option.label, forSegment: index)
      if option.value == element.selection {
        selectedSegment = index
      }
    }
    control.selectedSegment = selectedSegment
    control.invalidateIntrinsicContentSize()
  }

  static func dismantleNSView(_ control: NSSegmentedControl, coordinator: Coordinator) {
    control.target = nil
    control.action = nil
  }

  final class Coordinator: NSObject {
    private let actionSink: QuickGUIActionSink
    private var id: UInt64 = 0
    private var options: [QuickGUIPickerOption] = []
    private var hasValueChange = false

    init(actionSink: QuickGUIActionSink) {
      self.actionSink = actionSink
    }

    fileprivate func update(
      id: UInt64,
      options: [QuickGUIPickerOption],
      hasValueChange: Bool
    ) {
      self.id = id
      self.options = options
      self.hasValueChange = hasValueChange
    }

    @objc fileprivate func selectionChanged(_ sender: NSSegmentedControl) {
      let index = sender.selectedSegment
      guard hasValueChange, options.indices.contains(index) else { return }
      actionSink.sendValue(id, options[index].value)
    }
  }
}

private func quickGUIAppKitControlSize(_ size: SwiftUI.ControlSize) -> NSControl.ControlSize {
  switch size {
  case .mini: return .mini
  case .small: return .small
  case .large: return .large
  case .extraLarge:
    if #available(macOS 26.0, *) {
      return .extraLarge
    }
    return .large
  default: return .regular
  }
}

private final class QuickGUIPopoverAnchorView: NSView {
  var didMoveToWindow: (() -> Void)?

  override func viewDidMoveToWindow() {
    super.viewDidMoveToWindow()
    didMoveToWindow?()
  }

  override func hitTest(_ point: NSPoint) -> NSView? {
    nil
  }
}

private struct QuickGUIPopoverAnchorRepresentable: NSViewRepresentable {
  let id: UInt64
  let isPresented: Bool
  let attachmentAnchor: String?
  let arrowEdge: String?
  let content: AnyView
  let actionSink: QuickGUIActionSink

  func makeCoordinator() -> Coordinator {
    Coordinator(id: id, actionSink: actionSink)
  }

  func makeNSView(context: Context) -> QuickGUIPopoverAnchorView {
    let view = QuickGUIPopoverAnchorView(frame: .zero)
    view.setAccessibilityElement(false)
    view.didMoveToWindow = { [weak coordinator = context.coordinator] in
      coordinator?.reconcile()
    }
    context.coordinator.anchor = view
    return view
  }

  func updateNSView(_ nsView: QuickGUIPopoverAnchorView, context: Context) {
    context.coordinator.update(
      anchor: nsView,
      isPresented: isPresented,
      attachmentAnchor: attachmentAnchor,
      arrowEdge: arrowEdge,
      content: content
    )
  }

  static func dismantleNSView(_ nsView: QuickGUIPopoverAnchorView, coordinator: Coordinator) {
    nsView.didMoveToWindow = nil
    coordinator.detach()
  }

  final class Coordinator: NSObject, NSPopoverDelegate {
    fileprivate weak var anchor: QuickGUIPopoverAnchorView?
    private let id: UInt64
    private let actionSink: QuickGUIActionSink
    private var expectedPresented = false
    private var attachmentAnchor: String?
    private var arrowEdge: String?
    private var content = AnyView(EmptyView())
    private var popover: NSPopover?
    private var contentController: NSHostingController<AnyView>?

    init(id: UInt64, actionSink: QuickGUIActionSink) {
      self.id = id
      self.actionSink = actionSink
    }

    fileprivate func update(
      anchor: QuickGUIPopoverAnchorView,
      isPresented: Bool,
      attachmentAnchor: String?,
      arrowEdge: String?,
      content: AnyView
    ) {
      self.anchor = anchor
      self.expectedPresented = isPresented
      self.attachmentAnchor = attachmentAnchor
      self.arrowEdge = arrowEdge
      self.content = content
      reconcile()
    }

    fileprivate func reconcile() {
      guard expectedPresented else {
        popover?.close()
        return
      }
      guard let anchor, anchor.window != nil else { return }

      let controller: NSHostingController<AnyView>
      if let contentController {
        contentController.rootView = content
        controller = contentController
      } else {
        controller = NSHostingController(rootView: content)
        if #available(macOS 13.0, *) {
          controller.sizingOptions = [.preferredContentSize]
        }
        contentController = controller
      }

      let popover: NSPopover
      if let current = self.popover {
        popover = current
      } else {
        popover = NSPopover()
        popover.behavior = .transient
        popover.animates = true
        popover.delegate = self
        self.popover = popover
      }
      popover.contentViewController = controller
      controller.view.layoutSubtreeIfNeeded()
      let fittingSize = controller.view.fittingSize
      if fittingSize.width.isFinite, fittingSize.height.isFinite,
        fittingSize.width > 0, fittingSize.height > 0
      {
        popover.contentSize = fittingSize
      }
      guard !popover.isShown else { return }
      popover.show(
        relativeTo: quickGUIPopoverAnchorRect(attachmentAnchor, in: anchor.bounds),
        of: anchor,
        preferredEdge: quickGUIPopoverPreferredEdge(arrowEdge)
      )
    }

    fileprivate func detach() {
      expectedPresented = false
      popover?.delegate = nil
      popover?.close()
      popover = nil
      contentController = nil
      anchor = nil
    }

    func popoverDidClose(_ notification: Notification) {
      guard expectedPresented else { return }
      expectedPresented = false
      actionSink.sendPresentation(id, false)
    }
  }
}

private struct QuickGUIElementGroup: View {
  let elements: [QuickGUIElement]
  @ObservedObject var store: QuickGUIElementStore
  let actionSink: QuickGUIActionSink

  var body: some View {
    VStack(spacing: 8) {
      ForEach(elements) { element in
        QuickGUIElementView(
          element: element,
          store: store,
          actionSink: actionSink
        )
      }
    }
  }
}

private struct QuickGUIElementView: View {
  let element: QuickGUIElement
  @ObservedObject var store: QuickGUIElementStore
  let actionSink: QuickGUIActionSink

  var body: some View {
    content
  }

  private var content: AnyView {
    switch element.type {
    case "button":
      return swiftUIButton(element)
    case "slider":
      return swiftUISlider(element)
    case "toggle":
      return swiftUIToggle(element)
    case "progressView":
      return swiftUIProgressView(element)
    case "stepper":
      return swiftUIStepper(element)
    case "textField":
      return swiftUITextField(element)
    case "picker":
      return swiftUIPicker(element)
    case "datePicker":
      return swiftUIDatePicker(element)
    case "colorPicker":
      return swiftUIColorPicker(element)
    case "gauge":
      return swiftUIGauge(element)
    case "quickGuiHost":
      var result = AnyView(QuickGUIEmbeddedRepresentable(store: store, id: element.id))
      if element.width != nil || element.height != nil {
        let width = element.width.map { CGFloat($0) }
        let height = element.height.map { CGFloat($0) }
        result = AnyView(
          result.frame(
            width: width,
            height: height
          )
        )
      }
      if let testID = element.testID, !testID.isEmpty {
        result = AnyView(result.accessibilityIdentifier(testID))
      }
      return result
    case "popover":
      return swiftUIPopover(element)
    default:
      return AnyView(EmptyView())
    }
  }

  private func swiftUIButton(_ element: QuickGUIElement) -> AnyView {
    let role: ButtonRole? = switch element.role {
    case "cancel": .cancel
    case "destructive": .destructive
    default: nil
    }
    let label = element.label ?? ""
    let button = Button(role: role) {
      // Always cross the native boundary for an actual SwiftUI Button activation. Rust/JS owns
      // listener presence and drops an event whose target no longer has an onPress handler. This
      // also avoids making a retained SwiftUI closure stale when listener props change in place.
      actionSink.sendAction(element.id)
    } label: {
      if element.label != nil, let systemImage = element.systemImage, !systemImage.isEmpty {
        Label(label, systemImage: systemImage)
      } else {
        Text(label)
      }
    }

    return decorate(AnyView(button), with: element)
  }

  private func swiftUISlider(_ element: QuickGUIElement) -> AnyView {
    let bounds = numericBounds(element, defaultMinimum: 0, defaultMaximum: 1)
    let value = Binding<Double>(
      get: { min(max(element.value ?? bounds.lowerBound, bounds.lowerBound), bounds.upperBound) },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, String(next))
      }
    )
    let label = element.label ?? ""
    let slider: AnyView
    if let step = element.step, step.isFinite, step > 0 {
      slider = AnyView(
        Slider(value: value, in: bounds, step: step) {
          Text(label)
        }
      )
    } else {
      slider = AnyView(
        Slider(value: value, in: bounds) {
          Text(label)
        }
      )
    }
    return decorate(slider, with: element)
  }

  private func swiftUIToggle(_ element: QuickGUIElement) -> AnyView {
    let isOn = Binding<Bool>(
      get: { element.isOn ?? false },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, next ? "true" : "false")
      }
    )
    let toggle = Toggle(isOn: isOn) {
      Text(element.label ?? "")
    }
    return decorate(AnyView(toggle), with: element)
  }

  private func swiftUIProgressView(_ element: QuickGUIElement) -> AnyView {
    let label = element.label ?? ""
    let progress: AnyView
    if let value = element.value {
      let total = element.total.flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? 1
      if let currentValueLabel = element.currentValueLabel, !currentValueLabel.isEmpty {
        progress = AnyView(
          ProgressView(value: value, total: total) {
            Text(label)
          } currentValueLabel: {
            Text(currentValueLabel)
          }
        )
      } else {
        progress = AnyView(
          ProgressView(value: value, total: total) {
            Text(label)
          }
        )
      }
    } else {
      progress = AnyView(ProgressView {
        Text(label)
      })
    }
    return decorate(progress, with: element)
  }

  private func swiftUIStepper(_ element: QuickGUIElement) -> AnyView {
    let bounds = numericBounds(element, defaultMinimum: 0, defaultMaximum: 100)
    let value = Binding<Double>(
      get: { min(max(element.value ?? bounds.lowerBound, bounds.lowerBound), bounds.upperBound) },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, String(next))
      }
    )
    let step = element.step.flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? 1
    let stepper = Stepper(value: value, in: bounds, step: step) {
      Text(element.label ?? "")
    }
    return decorate(AnyView(stepper), with: element)
  }

  private func swiftUITextField(_ element: QuickGUIElement) -> AnyView {
    let text = Binding<String>(
      get: { element.text ?? "" },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, next)
      }
    )
    let field: AnyView
    if element.secure ?? false {
      field = AnyView(SecureField(element.placeholder ?? "", text: text))
    } else {
      field = AnyView(TextField(element.placeholder ?? "", text: text))
    }
    let submitting = field.onSubmit {
      guard element.hasSubmit ?? false else { return }
      actionSink.sendSubmit(element.id)
    }
    return decorate(AnyView(submitting), with: element)
  }

  private func swiftUIPicker(_ element: QuickGUIElement) -> AnyView {
    let selection = Binding<String>(
      get: { element.selection ?? "" },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, next)
      }
    )
    let picker = Picker(selection: selection) {
      ForEach(element.options ?? [], id: \.value) { option in
        Group {
          if let systemImage = option.systemImage, !systemImage.isEmpty {
            Label(option.label, systemImage: systemImage)
          } else {
            Text(option.label)
          }
        }
        .tag(option.value)
        .disabled(option.disabled)
      }
    } label: {
      Text(element.label ?? "")
    }
    var result: AnyView
    switch element.style {
    case "menu": result = AnyView(picker.pickerStyle(.menu))
    case "segmented": result = AnyView(picker.pickerStyle(.segmented))
    case "tabs":
      #if compiler(>=6.4)
        if #available(macOS 27.0, *) {
          result = AnyView(picker.pickerStyle(.tabs))
        } else {
          result = quickGUITabsPickerFallback(element)
        }
      #else
        result = quickGUITabsPickerFallback(element)
      #endif
    case "radioGroup": result = AnyView(picker.pickerStyle(.radioGroup))
    case "inline": result = AnyView(picker.pickerStyle(.inline))
    default: result = AnyView(picker.pickerStyle(.automatic))
    }
    if element.label == nil {
      result = AnyView(result.labelsHidden())
    }
    return decorate(result, with: element)
  }

  private func quickGUITabsPickerFallback(_ element: QuickGUIElement) -> AnyView {
    let tabs = QuickGUITabsPickerRepresentable(element: element, actionSink: actionSink)
    if #available(macOS 27.0, *) {
      return AnyView(tabs)
    }
    if #available(macOS 26.0, *) {
      return AnyView(
        tabs
          .padding(3)
          .glassEffect(.regular.interactive(), in: Capsule())
      )
    }
    return AnyView(tabs)
  }

  private func swiftUIDatePicker(_ element: QuickGUIElement) -> AnyView {
    let components: DatePickerComponents = switch element.components {
    case "date": [.date]
    case "hourAndMinute": [.hourAndMinute]
    default: [.date, .hourAndMinute]
    }
    var minimum = element.minimum.flatMap { $0.isFinite ? Date(timeIntervalSince1970: $0) : nil }
    var maximum = element.maximum.flatMap { $0.isFinite ? Date(timeIntervalSince1970: $0) : nil }
    if let lower = minimum, let upper = maximum, upper < lower {
      swap(&minimum, &maximum)
    }
    let value = Binding<Date>(
      get: {
        let raw = element.value.flatMap { $0.isFinite ? Date(timeIntervalSince1970: $0) : nil }
          ?? Date(timeIntervalSince1970: 0)
        return min(max(raw, minimum ?? raw), maximum ?? raw)
      },
      set: { next in
        guard element.hasValueChange ?? false else { return }
        actionSink.sendValue(element.id, String(next.timeIntervalSince1970))
      }
    )
    let picker: AnyView
    if let minimum, let maximum {
      picker = AnyView(
        DatePicker(
          element.label ?? "",
          selection: value,
          in: minimum...maximum,
          displayedComponents: components
        )
      )
    } else if let minimum {
      picker = AnyView(
        DatePicker(
          element.label ?? "",
          selection: value,
          in: minimum...,
          displayedComponents: components
        )
      )
    } else if let maximum {
      picker = AnyView(
        DatePicker(
          element.label ?? "",
          selection: value,
          in: ...maximum,
          displayedComponents: components
        )
      )
    } else {
      picker = AnyView(
        DatePicker(
          element.label ?? "",
          selection: value,
          displayedComponents: components
        )
      )
    }
    var result: AnyView = switch element.style {
    case "field": AnyView(picker.datePickerStyle(.field))
    case "graphical": AnyView(picker.datePickerStyle(.graphical))
    case "stepperField": AnyView(picker.datePickerStyle(.stepperField))
    default: AnyView(picker.datePickerStyle(.automatic))
    }
    if element.label == nil {
      result = AnyView(result.labelsHidden())
    }
    return decorate(result, with: element)
  }

  private func swiftUIColorPicker(_ element: QuickGUIElement) -> AnyView {
    let supportsOpacity = element.supportsOpacity ?? true
    let selection = Binding<Color>(
      get: { element.selection.flatMap(quickGUIColor) ?? .accentColor },
      set: { next in
        guard element.hasValueChange ?? false,
          let encoded = quickGUIHexColor(next, supportsOpacity: supportsOpacity)
        else { return }
        actionSink.sendValue(element.id, encoded)
      }
    )
    var result = AnyView(
      ColorPicker(
        element.label ?? "",
        selection: selection,
        supportsOpacity: supportsOpacity
      )
    )
    if element.label == nil {
      result = AnyView(result.labelsHidden())
    }
    return decorate(result, with: element)
  }

  private func swiftUIGauge(_ element: QuickGUIElement) -> AnyView {
    let bounds = numericBounds(element, defaultMinimum: 0, defaultMaximum: 1)
    let value = min(max(element.value ?? bounds.lowerBound, bounds.lowerBound), bounds.upperBound)
    let gauge = Gauge(value: value, in: bounds) {
      Text(element.label ?? "")
    } currentValueLabel: {
      if let label = element.currentValueLabel {
        Text(label)
      }
    } minimumValueLabel: {
      if let label = element.minimumValueLabel {
        Text(label)
      }
    } maximumValueLabel: {
      if let label = element.maximumValueLabel {
        Text(label)
      }
    }
    let result: AnyView = switch element.style {
    case "accessoryCircular": AnyView(gauge.gaugeStyle(.accessoryCircular))
    case "accessoryCircularCapacity": AnyView(gauge.gaugeStyle(.accessoryCircularCapacity))
    case "accessoryLinear": AnyView(gauge.gaugeStyle(.accessoryLinear))
    case "accessoryLinearCapacity": AnyView(gauge.gaugeStyle(.accessoryLinearCapacity))
    default: AnyView(gauge.gaugeStyle(.automatic))
    }
    return decorate(result, with: element)
  }

  private func numericBounds(
    _ element: QuickGUIElement,
    defaultMinimum: Double,
    defaultMaximum: Double
  ) -> ClosedRange<Double> {
    let minimum = element.minimum.flatMap { $0.isFinite ? $0 : nil } ?? defaultMinimum
    let proposedMaximum = element.maximum.flatMap { $0.isFinite ? $0 : nil } ?? defaultMaximum
    let maximum = proposedMaximum > minimum ? proposedMaximum : minimum + 1
    return minimum...maximum
  }

  private func decorate(_ view: AnyView, with element: QuickGUIElement) -> AnyView {
    var result = view
    for modifier in element.modifiers ?? [] {
      result = apply(modifier, to: result)
    }
    if let testID = element.testID, !testID.isEmpty {
      result = AnyView(result.accessibilityIdentifier(testID))
    }
    return result
  }

  private func swiftUIPopover(_ element: QuickGUIElement) -> AnyView {
    let trigger = QuickGUIElementGroup(
      elements: element.trigger ?? [],
      store: store,
      actionSink: actionSink
    )
    let popoverContent = QuickGUIElementGroup(
      elements: element.content ?? [],
      store: store,
      actionSink: actionSink
    )
    let anchor = QuickGUIPopoverAnchorRepresentable(
      id: element.id,
      isPresented: element.isPresented ?? false,
      attachmentAnchor: element.attachmentAnchor,
      arrowEdge: element.arrowEdge,
      content: AnyView(popoverContent),
      actionSink: actionSink
    )
    var result = AnyView(
      trigger.background(anchor)
    )
    if let testID = element.testID, !testID.isEmpty {
      result = AnyView(result.accessibilityIdentifier(testID))
    }
    return result
  }

  private func apply(_ modifier: QuickGUIModifier, to view: AnyView) -> AnyView {
    switch modifier.type {
    case "buttonStyle":
      switch modifier.style {
      case "bordered": return AnyView(view.buttonStyle(.bordered))
      case "borderedProminent": return AnyView(view.buttonStyle(.borderedProminent))
      case "borderless": return AnyView(view.buttonStyle(.borderless))
      case "plain": return AnyView(view.buttonStyle(.plain))
      case "glass":
        #if compiler(>=6.2)
          if #available(macOS 26.0, *) {
            return AnyView(view.buttonStyle(.glass))
          }
        #endif
        return AnyView(view.buttonStyle(.bordered))
      case "glassProminent":
        #if compiler(>=6.2)
          if #available(macOS 26.0, *) {
            return AnyView(view.buttonStyle(.glassProminent))
          }
        #endif
        return AnyView(view.buttonStyle(.borderedProminent))
      default: return AnyView(view.buttonStyle(.automatic))
      }
    case "buttonBorderShape":
      switch modifier.shape {
      case "capsule":
        if #available(macOS 14.0, *) {
          return AnyView(view.buttonBorderShape(.capsule))
        }
        return AnyView(view.buttonBorderShape(.roundedRectangle))
      case "roundedRectangle":
        if #available(macOS 14.0, *), let radius = modifier.cornerRadius {
          return AnyView(view.buttonBorderShape(.roundedRectangle(radius: CGFloat(radius))))
        }
        return AnyView(view.buttonBorderShape(.roundedRectangle))
      case "circle":
        if #available(macOS 14.0, *) {
          return AnyView(view.buttonBorderShape(.circle))
        }
        return AnyView(view.buttonBorderShape(.roundedRectangle))
      default: return AnyView(view.buttonBorderShape(.automatic))
      }
    case "controlSize":
      switch modifier.size {
      case "mini": return AnyView(view.controlSize(.mini))
      case "small": return AnyView(view.controlSize(.small))
      case "large": return AnyView(view.controlSize(.large))
      case "extraLarge":
        if #available(macOS 15.0, *) {
          return AnyView(view.controlSize(.extraLarge))
        }
        return AnyView(view.controlSize(.large))
      default: return AnyView(view.controlSize(.regular))
      }
    case "labelStyle":
      switch modifier.style {
      case "iconOnly": return AnyView(view.labelStyle(.iconOnly))
      case "titleAndIcon": return AnyView(view.labelStyle(.titleAndIcon))
      case "titleOnly": return AnyView(view.labelStyle(.titleOnly))
      default: return AnyView(view.labelStyle(.automatic))
      }
    case "tint":
      if let color = modifier.color.flatMap(quickGUIColor) {
        return AnyView(view.tint(color))
      }
      return view
    case "disabled": return AnyView(view.disabled(modifier.disabled ?? true))
    default: return view
    }
  }
}

private func quickGUIPopoverAnchorRect(_ value: String?, in bounds: NSRect) -> NSRect {
  let point: NSPoint
  switch value {
  case "top": point = NSPoint(x: bounds.midX, y: bounds.maxY)
  case "bottom": point = NSPoint(x: bounds.midX, y: bounds.minY)
  case "leading": point = NSPoint(x: bounds.minX, y: bounds.midY)
  case "trailing": point = NSPoint(x: bounds.maxX, y: bounds.midY)
  default: point = NSPoint(x: bounds.midX, y: bounds.midY)
  }
  return NSRect(origin: point, size: NSSize(width: 1, height: 1))
}

private func quickGUIPopoverPreferredEdge(_ value: String?) -> NSRectEdge {
  switch value {
  case "top": return .minY
  case "leading": return .maxX
  case "trailing": return .minX
  default: return .maxY
  }
}

private struct QuickGUIRootView: View {
  @ObservedObject var store: QuickGUIElementStore
  let actionSink: QuickGUIActionSink

  var body: some View {
    QuickGUIElementGroup(
      elements: store.elements,
      store: store,
      actionSink: actionSink
    )
      .padding(store.effectInset)
      .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
      // QuickGUI places the host exactly where its element is laid out, including under a
      // transparent titlebar. SwiftUI must not move the content out of the window's safe area
      // or report that inset in the fitting size, which would grow the host on every build.
      .ignoresSafeArea()
  }
}

private func quickGUIColor(_ value: String) -> Color? {
  switch value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
  case "primary": return .primary
  case "secondary": return .secondary
  case "red": return .red
  case "orange": return .orange
  case "yellow": return .yellow
  case "green": return .green
  case "blue": return .blue
  case "purple": return .purple
  case "pink": return .pink
  case "white": return .white
  case "gray", "grey": return .gray
  case "black": return .black
  case "clear", "transparent": return .clear
  default: break
  }

  let value = value.trimmingCharacters(in: .whitespacesAndNewlines)
  guard value.hasPrefix("#") else { return nil }
  let hex = String(value.dropFirst())
  let expanded: String
  if hex.count == 3 || hex.count == 4 {
    expanded = hex.map { "\($0)\($0)" }.joined()
  } else {
    expanded = hex
  }
  guard (expanded.count == 6 || expanded.count == 8),
    let number = UInt64(expanded, radix: 16)
  else {
    return nil
  }
  let hasAlpha = expanded.count == 8
  let red = Double((number >> (hasAlpha ? 24 : 16)) & 0xff) / 255
  let green = Double((number >> (hasAlpha ? 16 : 8)) & 0xff) / 255
  let blue = Double((number >> (hasAlpha ? 8 : 0)) & 0xff) / 255
  let alpha = hasAlpha ? Double(number & 0xff) / 255 : 1
  return Color(red: red, green: green, blue: blue, opacity: alpha)
}

private func quickGUIHexColor(_ color: Color, supportsOpacity: Bool) -> String? {
  guard let converted = NSColor(color).usingColorSpace(.sRGB) else { return nil }
  let red = Int((min(max(converted.redComponent, 0), 1) * 255).rounded())
  let green = Int((min(max(converted.greenComponent, 0), 1) * 255).rounded())
  let blue = Int((min(max(converted.blueComponent, 0), 1) * 255).rounded())
  let alpha = supportsOpacity
    ? Int((min(max(converted.alphaComponent, 0), 1) * 255).rounded())
    : 255
  return String(format: "#%02x%02x%02x%02x", red, green, blue, alpha)
}

private final class QuickGUIHostHandle {
  let actionSink: QuickGUIActionSink
  let store: QuickGUIElementStore
  let view: NSHostingView<QuickGUIRootView>

  init(
    context: UnsafeMutableRawPointer?,
    actionCallback: QuickGUIActionCallback?,
    presentationCallback: QuickGUIPresentationCallback?,
    valueCallback: QuickGUIValueCallback?,
    submitCallback: QuickGUISubmitCallback?
  ) {
    let actionSink = QuickGUIActionSink(
      context: context,
      actionCallback: actionCallback,
      presentationCallback: presentationCallback,
      valueCallback: valueCallback,
      submitCallback: submitCallback
    )
    let store = QuickGUIElementStore()
    self.actionSink = actionSink
    self.store = store
    self.view = NSHostingView(rootView: QuickGUIRootView(store: store, actionSink: actionSink))
    self.view.sizingOptions = [.intrinsicContentSize]
  }

  func update(json: UnsafePointer<CChar>) -> Bool {
    guard let data = String(cString: json).data(using: .utf8),
      let elements = try? JSONDecoder().decode([QuickGUIElement].self, from: data)
    else {
      return false
    }
    store.updateElements(elements)
    view.invalidateIntrinsicContentSize()
    view.layoutSubtreeIfNeeded()
    return true
  }
}

@_cdecl("quickgui_swift_ui_host_create")
func quickGUISwiftUIHostCreate(
  _ context: UnsafeMutableRawPointer?,
  _ actionCallback: QuickGUIActionCallback?,
  _ presentationCallback: QuickGUIPresentationCallback?,
  _ valueCallback: QuickGUIValueCallback?,
  _ submitCallback: QuickGUISubmitCallback?
) -> UnsafeMutableRawPointer? {
  precondition(Thread.isMainThread)
  return Unmanaged.passRetained(
    QuickGUIHostHandle(
      context: context,
      actionCallback: actionCallback,
      presentationCallback: presentationCallback,
      valueCallback: valueCallback,
      submitCallback: submitCallback
    )
  ).toOpaque()
}

@_cdecl("quickgui_swift_ui_host_view")
func quickGUISwiftUIHostView(
  _ opaqueHandle: UnsafeMutableRawPointer
) -> UnsafeMutableRawPointer {
  precondition(Thread.isMainThread)
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  return Unmanaged.passUnretained(handle.view).toOpaque()
}

@_cdecl("quickgui_swift_ui_host_update")
func quickGUISwiftUIHostUpdate(
  _ opaqueHandle: UnsafeMutableRawPointer,
  _ json: UnsafePointer<CChar>
) -> Bool {
  precondition(Thread.isMainThread)
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  return handle.update(json: json)
}

@_cdecl("quickgui_swift_ui_host_set_embedded_view")
func quickGUISwiftUIHostSetEmbeddedView(
  _ opaqueHandle: UnsafeMutableRawPointer,
  _ id: UInt64,
  _ opaqueView: UnsafeMutableRawPointer,
  _ width: Double,
  _ height: Double
) -> Bool {
  precondition(Thread.isMainThread)
  guard width.isFinite, height.isFinite, width > 0, height > 0 else { return false }
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  let view = Unmanaged<NSView>.fromOpaque(opaqueView).takeUnretainedValue()
  handle.store.setEmbeddedView(
    id,
    view: view,
    size: NSSize(width: width, height: height)
  )
  handle.view.invalidateIntrinsicContentSize()
  return true
}

@_cdecl("quickgui_swift_ui_host_remove_embedded_view")
func quickGUISwiftUIHostRemoveEmbeddedView(
  _ opaqueHandle: UnsafeMutableRawPointer,
  _ id: UInt64
) {
  precondition(Thread.isMainThread)
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  handle.store.removeEmbeddedView(id)
  handle.view.invalidateIntrinsicContentSize()
}

@_cdecl("quickgui_swift_ui_host_fitting_size")
func quickGUISwiftUIHostFittingSize(
  _ opaqueHandle: UnsafeMutableRawPointer,
  _ width: UnsafeMutablePointer<Double>,
  _ height: UnsafeMutablePointer<Double>
) {
  precondition(Thread.isMainThread)
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  handle.view.layoutSubtreeIfNeeded()
  let size = handle.view.fittingSize
  width.pointee = size.width
  height.pointee = size.height
}

@_cdecl("quickgui_swift_ui_host_effect_inset")
func quickGUISwiftUIHostEffectInset(_ opaqueHandle: UnsafeMutableRawPointer) -> Double {
  precondition(Thread.isMainThread)
  let handle = Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).takeUnretainedValue()
  return Double(handle.store.effectInset)
}

@_cdecl("quickgui_swift_ui_host_release")
func quickGUISwiftUIHostRelease(_ opaqueHandle: UnsafeMutableRawPointer) {
  precondition(Thread.isMainThread)
  Unmanaged<QuickGUIHostHandle>.fromOpaque(opaqueHandle).release()
}
