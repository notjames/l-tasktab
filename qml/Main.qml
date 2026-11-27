import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import l_tasktabs

ApplicationWindow
{
  id: root

  width:         960
  height:        640
  minimumWidth:  720
  minimumHeight: 480
  visible:       true
  title:         AppController.appName()
  color:         palette.window

  property var    sessions:         []
  property var    elevationModes:   []
  property var    running:          []
  property int    selectedSession:  -1
  property bool   newSessionMode:   false
  property int    activeRuntime:    -1
  property bool   dirty:            false
  property string statusText:       ""
  property string editSessionId:    ""
  property string terminalText:     ""
  // Per-runtime HTML so background tabs keep updating and tab switches
  // never show another session's buffer.
  property var    htmlById:         ({})

  readonly property string activeRuntimeId:
    (activeRuntime >= 0 && activeRuntime < running.length)
      ? running[activeRuntime].runtimeId
      : ""

  onActiveRuntimeChanged:
  {
    AppController.setFocusedRuntime(activeRuntimeId)
    if (activeRuntimeId)
    {
      htmlById[activeRuntimeId]
        ? applyTerminalHtml(htmlById[activeRuntimeId])
        : refreshTerminal()
    }
    resizeDebounce.restart()
    Qt.callLater(() => termInput.forceActiveFocus())
  }

  onWidthChanged:  resizeDebounce.restart()
  onHeightChanged: resizeDebounce.restart()

  function refreshConfig()
  {
    sessions       = AppController.sessionsJson()
    quickConfigured.model = sessions
    elevationModes = AppController.elevationModesJson()
    dirty          = AppController.isDirty()
    statusText     = AppController.statusMessage()
    hideOnCloseBox.checked = AppController.hideOnClose()
    autostartBox.checked   = AppController.autostart()
    quickManageBox.checked = AppController.quickManageEnabled()

    const mode = AppController.elevationMode()
    for (let i = 0; i < elevationModeBox.count; ++i)
    {
      elevationModeBox.model[i].id === mode
        && (elevationModeBox.currentIndex = i, true)
        && (i = elevationModeBox.count)
    }

    if (!sessions.length)
    {
      selectedSession = -1
      newSessionMode = false
      clearEditor()
    }
    else if (!newSessionMode)
    {
      // Configured sessions are the default editor view; New is an explicit
      // mode entered through the button beside the session list.
      selectedSession = (selectedSession < 0 || selectedSession >= sessions.length)
        ? 0
        : selectedSession
      loadEditor(sessions[selectedSession])
    }
  }

  function refreshRunning()
  {
    const prev = activeRuntimeId
    running = AppController.runningSessionsJson()
    quickRunning.model = running
    let idx = -1
    for (let i = 0; i < running.length; ++i)
      running[i].runtimeId === prev && (idx = i)
    activeRuntime = (idx >= 0) ? idx : (running.length ? 0 : -1)
  }

  function refreshTerminal()
  {
    // Prefer cached snapshot (already rendered off the UI thread when possible).
    const html = activeRuntimeId
      ? AppController.terminalSnapshot(activeRuntimeId)
      : ""
    activeRuntimeId && html && storeHtml(activeRuntimeId, html)
    applyTerminalHtml(html)
  }

  function storeHtml(runtimeId, html)
  {
    const next = Object.assign({}, htmlById)
    next[runtimeId] = html
    htmlById = next
  }

  function applyTerminalHtml(html)
  {
    if (html === terminalText)
      return
    terminalText = html
  }

  function selectRuntime(runtimeId)
  {
    for (let i = 0; i < running.length; ++i)
    {
      if (running[i].runtimeId === runtimeId)
      {
        activeRuntime = i
        return
      }
    }
  }

  function loadEditor(s)
  {
    editSessionId         = s.id || ""
    editName.text         = s.name || ""
    editCommand.text      = s.command || ""
    editCwd.text          = s.working_directory || "~"
    editAutostart.checked = !!s.autostart
    editElevated.checked  = !!s.elevated
  }

  function clearEditor()
  {
    editSessionId         = ""
    editName.text         = ""
    editCommand.text      = ""
    editCwd.text          = "~"
    editAutostart.checked = false
    editElevated.checked  = false
  }

  function editorPayload()
  {
    return {
      id:                editSessionId,
      name:              editName.text.trim(),
      command:           editCommand.text.trim(),
      working_directory: editCwd.text.trim() || "~",
      autostart:         editAutostart.checked,
      elevated:          editElevated.checked,
      backend:           "builtin"
    }
  }

  function saveSessionEditor()
  {
    const r = AppController.upsertSessionJson(editorPayload())
    if (r.indexOf("ERR:") === 0)
    {
      statusText = r.slice(4)
      return ""
    }
    editSessionId = r
    const err = AppController.saveConfig()
    err
      ? (statusText = err)
      : (
          newSessionMode = false,
          selectedSession = sessions.findIndex(s => s.id === editSessionId),
          statusText = qsTr("Session saved"),
          refreshConfig()
        )
    return err ? "" : r
  }

  function saveAppSettings()
  {
    AppController.setHideOnClose(hideOnCloseBox.checked)
    AppController.setAutostart(autostartBox.checked)
    AppController.setQuickManageEnabled(quickManageBox.checked)
    if (elevationModeBox.currentIndex >= 0 && elevationModeBox.model[elevationModeBox.currentIndex])
      AppController.setElevationMode(elevationModeBox.model[elevationModeBox.currentIndex].id)
    const err = AppController.saveConfig()
    err
      ? (statusText = err)
      : (statusText = qsTr("Settings saved"), refreshConfig())
  }

  function sendKey(event)
  {
    if (!activeRuntimeId)
      return

    let data = ""
    const ctrl = !!(event.modifiers & Qt.ControlModifier)
    const alt  = !!(event.modifiers & Qt.AltModifier)

    // Ctrl+A .. Ctrl+Z (critical for TUIs: Ctrl+C, Ctrl+Z, Ctrl+L, …)
    if (ctrl && event.key >= Qt.Key_A && event.key <= Qt.Key_Z)
      data = String.fromCharCode(event.key - Qt.Key_A + 1)
    else if (ctrl && event.key === Qt.Key_Space)
      data = "\u0000"
    else if (ctrl && (event.key === Qt.Key_BracketLeft || event.key === Qt.Key_Escape))
      data = "\u001b"
    else switch (event.key)
    {
      case Qt.Key_Return:
      case Qt.Key_Enter:     data = "\r"; break
      case Qt.Key_Backspace: data = "\u007f"; break
      case Qt.Key_Tab:       data = "\t"; break
      case Qt.Key_Escape:    data = "\u001b"; break
      case Qt.Key_Up:        data = "\u001b[A"; break
      case Qt.Key_Down:      data = "\u001b[B"; break
      case Qt.Key_Right:     data = "\u001b[C"; break
      case Qt.Key_Left:      data = "\u001b[D"; break
      case Qt.Key_Home:      data = "\u001b[H"; break
      case Qt.Key_End:       data = "\u001b[F"; break
      case Qt.Key_PageUp:    data = "\u001b[5~"; break
      case Qt.Key_PageDown:  data = "\u001b[6~"; break
      case Qt.Key_Delete:    data = "\u001b[3~"; break
      case Qt.Key_Insert:    data = "\u001b[2~"; break
      case Qt.Key_F1:        data = "\u001bOP"; break
      case Qt.Key_F2:        data = "\u001bOQ"; break
      case Qt.Key_F3:        data = "\u001bOR"; break
      case Qt.Key_F4:        data = "\u001bOS"; break
      case Qt.Key_F5:        data = "\u001b[15~"; break
      case Qt.Key_F6:        data = "\u001b[17~"; break
      case Qt.Key_F7:        data = "\u001b[18~"; break
      case Qt.Key_F8:        data = "\u001b[19~"; break
      case Qt.Key_F9:        data = "\u001b[20~"; break
      case Qt.Key_F10:       data = "\u001b[21~"; break
      case Qt.Key_F11:       data = "\u001b[23~"; break
      case Qt.Key_F12:       data = "\u001b[24~"; break
      default:
        event.text && (data = event.text)
    }

    if (alt && data && data.length === 1 && data.charCodeAt(0) >= 32)
      data = "\u001b" + data

    if (data)
      AppController.writeTerminal(activeRuntimeId, data)
    event.accepted = true
  }

  function estimateTermSize()
  {
    if (termView.width < 8 || termView.height < 8)
      return
    const cw = Math.max(1, termMetrics.averageCharacterWidth)
    const ch = Math.max(1, termMetrics.height)
    const cols = Math.max(40, Math.floor((termView.width  - 4) / cw))
    const rows = Math.max(10, Math.floor((termView.height - 4) / ch))
    AppController.resizeAllTerminals(rows, cols)
  }

  function terminalMouseButton(button)
  {
    if (button === Qt.LeftButton)
      return 0
    if (button === Qt.MiddleButton)
      return 1
    if (button === Qt.RightButton)
      return 2
    return 0
  }

  function terminalMouseModifiers(modifiers)
  {
    let value = 0
    modifiers & Qt.ShiftModifier && (value |= 1)
    modifiers & Qt.AltModifier && (value |= 2)
    modifiers & Qt.ControlModifier && (value |= 4)
    return value
  }

  function sendTerminalMouse(kind, button, x, y, modifiers)
  {
    if (!activeRuntimeId)
      return false
    const content = termScroll.contentItem
    const cellX = Math.floor((x + content.contentX) / Math.max(1, termMetrics.averageCharacterWidth)) + 1
    const cellY = Math.floor((y + content.contentY) / Math.max(1, termMetrics.height)) + 1
    return AppController.sendTerminalMouse(
      activeRuntimeId, kind, terminalMouseButton(button), cellX, cellY,
      terminalMouseModifiers(modifiers))
  }

  Timer
  {
    id:       resizeDebounce
    interval: 80
    repeat:   false
    onTriggered: estimateTermSize()
  }

  function openQuickManage(x, y)
  {
    refreshRunning()
    refreshTerminal()
    quickManage.width  = 480
    quickManage.height = 460
    if (x >= 0 && y >= 0)
    {
      quickManage.x = Math.max(0, x - quickManage.width / 2)
      quickManage.y = Math.max(0, y - quickManage.height - 12)
    }
    else
    {
      quickManage.x = root.x + (root.width  - quickManage.width)  / 2
      quickManage.y = root.y + (root.height - quickManage.height) / 2
    }
    quickManage.show()
    quickManage.raise()
    quickManage.requestActivate()
  }

  onClosing: function (close)
  {
    AppController.hideOnClose() && (
      close.accepted = false,
      root.hide(),
      AppController.hideMainWindow()
    )
  }

  FontMetrics
  {
    id:   termMetrics
    font: termArea.font
  }

  Connections
  {
    target: AppController

    function onWindowVisibilityChanged(visible)
    {
      visible
        ? (root.show(), root.raise(), root.requestActivate())
        : root.hide()
    }

    function onQuitRequested()
    {
      Qt.quit()
    }

    function onConfigChanged()
    {
      root.refreshConfig()
    }

    function onRunningChanged()
    {
      root.refreshRunning()
    }

    function onSessionStarted(runtimeId)
    {
      // Align QML selection with the session the user just started so Rust
      // focus and the visible pane stay in sync (fixes blank/swapped panes).
      root.selectRuntime(runtimeId)
      const cached = root.htmlById[runtimeId]
      cached ? root.applyTerminalHtml(cached) : root.refreshTerminal()
    }

    function onTerminalUpdated(runtimeId, html)
    {
      // Always cache; only paint when this runtime is the active tab.
      root.storeHtml(runtimeId, html)
      if (runtimeId === root.activeRuntimeId)
        root.applyTerminalHtml(html)
    }

    function onQuickManageRequested(x, y)
    {
      root.openQuickManage(x, y)
    }
  }

  // —— Quick Manage (tray hover companion / middle-click) ——
  Window
  {
    id:      quickManage
    title:   qsTr("L-TaskTabs — Quick Manage")
    visible: false
    flags:   Qt.Tool | Qt.WindowStaysOnTopHint
    color:   palette.window

    ColumnLayout
    {
      anchors.fill:    parent
      anchors.margins: 10
      spacing:         8

      Label
      {
        text:      qsTr("Quick Manage · %1 running").arg(running.length)
        font.bold: true
      }

      ListView
      {
        id:                quickRunning
        Layout.fillWidth:  true
        Layout.preferredHeight: Math.min(160, Math.max(40, running.length * 40))
        clip:              true
        model:             running
        delegate: ItemDelegate
        {
          width: ListView.view.width
          text:  modelData.name
          highlighted: modelData.runtimeId === activeRuntimeId
          onClicked:
          {
            for (let i = 0; i < running.length; ++i)
              running[i].runtimeId === modelData.runtimeId && (activeRuntime = i)
            refreshTerminal()
          }

          RowLayout
          {
            anchors.right: parent.right
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4

            Button
            {
              text: qsTr("Open")
              onClicked:
              {
                AppController.showMainWindow()
                mainTabs.currentIndex = 0
                quickManage.hide()
              }
            }
            Button
            {
              text: qsTr("Stop")
              onClicked: AppController.stopRunningSession(modelData.runtimeId)
            }
          }
        }
      }

      Label
      {
        visible: running.length === 0
        text:    qsTr("No running terminals")
        opacity: 0.65
      }

      Label
      {
        text:      qsTr("Configured sessions")
        font.bold: true
        visible:   sessions.length > 0
      }

      ListView
      {
        id:                   quickConfigured
        Layout.fillWidth:     true
        Layout.preferredHeight: Math.min(100, sessions.length * 34)
        clip:                 true
        model:                sessions
        visible:              sessions.length > 0
        delegate: ItemDelegate
        {
          width: ListView.view.width
          height: 34
          text: modelData.name
          onClicked:
          {
            const result = AppController.startConfiguredSession(modelData.id)
            if (result.indexOf("failed") >= 0
              || result.indexOf("not started") >= 0
              || result.indexOf("Unknown") >= 0)
              statusText = result
          }
        }
      }

      Frame
      {
        Layout.fillWidth:  true
        Layout.fillHeight: true
        padding:           0
        background: Rectangle { color: "#1e1e2e" }

        TextEdit
        {
          id:         quickTerm
          anchors.fill: parent
          readOnly:   true
          wrapMode:   TextEdit.NoWrap
          textFormat: TextEdit.RichText
          text:       terminalText
          color:      "#cdd6f4"
          font.family: "monospace"
          font.pixelSize: 11
        }
      }

      RowLayout
      {
        Button
        {
          text: qsTr("Show main window")
          onClicked:
          {
            AppController.showMainWindow()
            quickManage.hide()
          }
        }
        Item { Layout.fillWidth: true }
        Button
        {
          text: qsTr("Close")
          onClicked: quickManage.hide()
        }
      }
    }
  }

  header: ToolBar
  {
    RowLayout
    {
      anchors.fill:        parent
      anchors.leftMargin:  12
      anchors.rightMargin: 12
      spacing:             8

      Label
      {
        text:      AppController.appName()
        font.bold: true
      }

      Label
      {
        text:    dirty ? qsTr("Unsaved changes") : qsTr("Saved")
        opacity: dirty ? 1.0 : 0.55
        color:   dirty ? palette.link : palette.windowText
      }

      Item { Layout.fillWidth: true }
    }
  }

  ColumnLayout
  {
    anchors.fill:    parent
    anchors.margins: 10
    spacing:         8

    TabBar
    {
      id:               mainTabs
      Layout.fillWidth: true
      currentIndex:     0

      TabButton { text: qsTr("Terminals") }
      TabButton { text: qsTr("Sessions") }
      TabButton { text: qsTr("Application Settings") }
    }

    StackLayout
    {
      Layout.fillWidth:  true
      Layout.fillHeight: true
      currentIndex:      mainTabs.currentIndex

      // 0 — Terminals
      ColumnLayout
      {
        spacing: 0

        TabBar
        {
          id:               termTabs
          Layout.fillWidth: true
          currentIndex:     activeRuntime
          spacing:          0
          visible:          running.length > 0

          Repeater
          {
            model: running
            TabButton
            {
              text:      modelData.name
              width:     implicitWidth + 8
              onClicked: activeRuntime = index
            }
          }
        }

        Frame
        {
          id:                termFrame
          Layout.fillWidth:  true
          Layout.fillHeight: true
          padding:           0

          background: Rectangle { color: "#1e1e2e" }

          Item
          {
            id:           termView
            anchors.fill: parent

            // Display surface only — must not steal keys from termInput.
            ScrollView
            {
              id:           termScroll
              anchors.fill: parent
              clip:         true
              focusPolicy:  Qt.NoFocus

              TextEdit
              {
                id:               termArea
                readOnly:         true
                wrapMode:         TextEdit.NoWrap
                textFormat:       TextEdit.RichText
                text:             terminalText
                color:            "#cdd6f4"
                selectedTextColor: "#1e1e2e"
                selectionColor:   "#89b4fa"
                font.family:      "monospace"
                font.pixelSize:   13
                activeFocusOnPress: false
                persistentSelection: true

                // Keep view pinned to bottom for TUIs without fighting key focus.
                onTextChanged:
                {
                  Qt.callLater(() => {
                    termScroll.ScrollBar.vertical.position =
                      Math.max(0, 1.0 - termScroll.ScrollBar.vertical.size)
                  })
                }
              }
            }

            // Dedicated key sink — always focused while on Terminals tab.
            Item
            {
              id:           termInput
              anchors.fill: parent
              focus:        true
              activeFocusOnTab: true

              Keys.onPressed: (event) => sendKey(event)
            }

            onWidthChanged:  resizeDebounce.restart()
            onHeightChanged: resizeDebounce.restart()

            MouseArea
            {
              anchors.fill:            parent
              acceptedButtons:         Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
              propagateComposedEvents: true
              onPressed: (mouse) =>
              {
                termInput.forceActiveFocus()
                mouse.accepted = sendTerminalMouse(
                  0, mouse.button, mouse.x, mouse.y, mouse.modifiers)
              }
              onReleased: (mouse) =>
              {
                mouse.accepted = sendTerminalMouse(
                  1, mouse.button, mouse.x, mouse.y, mouse.modifiers)
              }
              onPositionChanged: (mouse) =>
              {
                if (mouse.buttons !== Qt.NoButton)
                  mouse.accepted = sendTerminalMouse(
                    2, mouse.buttons & Qt.LeftButton ? Qt.LeftButton : mouse.buttons & Qt.MiddleButton ? Qt.MiddleButton : Qt.RightButton,
                    mouse.x, mouse.y, mouse.modifiers)
              }
              onWheel: (wheel) =>
              {
                const kind = wheel.angleDelta.y > 0 ? 3 : 4
                wheel.accepted = sendTerminalMouse(
                  kind, Qt.NoButton, wheel.x, wheel.y, wheel.modifiers)
              }
            }
          }

          Label
          {
            anchors.centerIn: parent
            visible:          running.length === 0
            color:            "#a6adc8"
            text:             qsTr("No running terminals.\nStart one from the Sessions tab.")
            horizontalAlignment: Text.AlignHCenter
          }
        }

        RowLayout
        {
          Layout.fillWidth: true
          spacing:          8

          Button
          {
            text:    qsTr("Stop tab")
            enabled: !!activeRuntimeId
            onClicked: AppController.stopRunningSession(activeRuntimeId)
          }

          Item { Layout.fillWidth: true }

          Label
          {
            text:             statusText
            opacity:          0.75
            elide:            Text.ElideRight
            Layout.fillWidth: true
          }
        }
      }

      // 1 — Sessions
      RowLayout
      {
        spacing: 12

        Frame
        {
          Layout.preferredWidth: 240
          Layout.fillHeight:     true

          ColumnLayout
          {
            anchors.fill:    parent
            anchors.margins: 8
            spacing:         8

            Label
            {
              text:      qsTr("Configured sessions")
              font.bold: true
            }

            ListView
            {
              Layout.fillWidth:  true
              Layout.fillHeight: true
              clip:              true
              model:             sessions
              currentIndex:      selectedSession
              delegate: ItemDelegate
              {
                width:       ListView.view.width
                text:        modelData.name || modelData.id
                highlighted: index === selectedSession
                onClicked:
                {
                  newSessionMode = false
                  selectedSession = index
                  loadEditor(sessions[index])
                }
              }
            }

            RowLayout
            {
              Button
              {
                text: qsTr("New")
                onClicked:
                {
                  selectedSession = -1
                  newSessionMode = true
                  clearEditor()
                  editName.forceActiveFocus()
                }
              }
              Button
              {
                text:    qsTr("Delete")
                enabled: selectedSession >= 0
                onClicked:
                {
                  AppController.removeSession(sessions[selectedSession].id)
                  const err = AppController.saveConfig()
                  err ? (statusText = err) : refreshConfig()
                }
              }
            }
          }
        }

        Frame
        {
          Layout.fillWidth:  true
          Layout.fillHeight: true

          ColumnLayout
          {
            anchors.fill:    parent
            anchors.margins: 12
            spacing:         8

            Label
            {
              text:      selectedSession < 0 ? qsTr("New session") : qsTr("Edit session")
              font.bold: true
            }

            GridLayout
            {
              columns:          2
              columnSpacing:    10
              rowSpacing:       8
              Layout.fillWidth: true

              Label { text: qsTr("Name") }
              TextField
              {
                id:               editName
                Layout.fillWidth: true
                placeholderText:  qsTr("syswatch")
              }

              Label { text: qsTr("Command") }
              TextField
              {
                id:               editCommand
                Layout.fillWidth: true
                placeholderText:  qsTr("htop   (empty = shell only)")
              }

              Label { text: qsTr("Working dir") }
              TextField
              {
                id:               editCwd
                Layout.fillWidth: true
                placeholderText:  "~"
              }

              Item {}
              CheckBox
              {
                id:   editAutostart
                text: qsTr("Autostart this session")
              }

              Item {}
              CheckBox
              {
                id:   editElevated
                text: qsTr("Elevated (sudo; autostart disabled)")
              }
            }

            Item { Layout.fillHeight: true }

            RowLayout
            {
              Layout.alignment: Qt.AlignRight
              spacing:          8

              Button
              {
                text:        qsTr("Save")
                highlighted: true
                enabled:     editName.text.trim().length > 0
                onClicked:   saveSessionEditor()
              }
              Button
              {
                text:    qsTr("Start")
                enabled: editName.text.trim().length > 0
                onClicked:
                {
                  const id = saveSessionEditor()
                  if (!id)
                    return
                  const result = AppController.startConfiguredSession(id)
                  const bad = result.indexOf("failed") >= 0
                    || result.indexOf("Unknown") >= 0
                    || result.indexOf("not started") >= 0
                    || result.length === 0
                  bad
                    ? (statusText = result || "Start failed")
                    : (mainTabs.currentIndex = 0, termView.forceActiveFocus())
                }
              }
            }
          }
        }
      }

      // 2 — Application Settings
      Frame
      {
        ColumnLayout
        {
          anchors.fill:    parent
          anchors.margins: 16
          spacing:         14

          Label
          {
            text:      qsTr("Application Settings")
            font.bold: true
          }

          CheckBox
          {
            id:   hideOnCloseBox
            text: qsTr("Hide to tray when closing the window")
          }

          CheckBox
          {
            id:   autostartBox
            text: qsTr("Autostart with desktop session (stored now; desktop entry in MVP 7)")
          }

          CheckBox
          {
            id:   quickManageBox
            text: qsTr("Enable tray Quick Manage (middle-click fallback)")
          }

          RowLayout
          {
            Label { text: qsTr("Elevation mode") }
            ComboBox
            {
              id:                    elevationModeBox
              Layout.preferredWidth: 260
              textRole:              "label"
              model:                 elevationModes
            }
          }

          Label
          {
            Layout.fillWidth: true
            wrapMode:         Text.WordWrap
            opacity:          0.7
            text:             qsTr("Tray: left-click toggles the window; hover shows a tooltip; middle-click opens the terminal selector popup.")
          }

          Item { Layout.fillHeight: true }

          Button
          {
            text:             qsTr("Save")
            highlighted:      true
            Layout.alignment: Qt.AlignRight
            onClicked:        saveAppSettings()
          }
        }
      }
    }

    RowLayout
    {
      Layout.fillWidth: true
      spacing:          8

      Label
      {
        Layout.fillWidth: true
        elide:            Text.ElideRight
        text:             qsTr("Config: %1").arg(AppController.configPath())
        opacity:          0.7
      }

      Button
      {
        text: qsTr("Reload")
        onClicked:
        {
          const err = AppController.reloadConfig()
          err && (statusText = err)
        }
      }
      Button
      {
        text: qsTr("Hide to tray")
        onClicked:
        {
          root.hide()
          AppController.hideMainWindow()
        }
      }
      Button
      {
        text: qsTr("Quit")
        onClicked: AppController.requestQuit()
      }
    }
  }

  // Keep key focus on the terminal input whenever Terminals is the active tab.
  Connections
  {
    target: mainTabs
    function onCurrentIndexChanged()
    {
      mainTabs.currentIndex === 0 && Qt.callLater(() => termInput.forceActiveFocus())
    }
  }

  Component.onCompleted:
  {
    AppController.startTray()
    AppController.showMainWindow()
    refreshConfig()
    refreshRunning()
    AppController.startAutostartSessions()
    resizeDebounce.restart()
    Qt.callLater(() => termInput.forceActiveFocus())
  }
}
