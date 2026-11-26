import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
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
  property int    activeRuntime:    -1
  property bool   dirty:            false
  property string statusText:       ""
  property string editSessionId:    ""
  property string terminalText:     ""

  readonly property string activeRuntimeId:
    (activeRuntime >= 0 && activeRuntime < running.length)
      ? running[activeRuntime].runtimeId
      : ""

  function refreshConfig()
  {
    sessions       = AppController.sessionsJson()
    elevationModes = AppController.elevationModesJson()
    dirty          = AppController.isDirty()
    statusText     = AppController.statusMessage()
    hideOnCloseBox.checked = AppController.hideOnClose()
    autostartBox.checked   = AppController.autostart()

    const mode = AppController.elevationMode()
    for (let i = 0; i < elevationModeBox.count; ++i)
    {
      elevationModeBox.model[i].id === mode
        && (elevationModeBox.currentIndex = i, true)
        && (i = elevationModeBox.count)
    }

    selectedSession = (selectedSession >= sessions.length)
      ? (sessions.length ? 0 : -1)
      : selectedSession
    selectedSession >= 0
      ? loadEditor(sessions[selectedSession])
      : clearEditor()
  }

  function refreshRunning()
  {
    const prev = activeRuntimeId
    running = AppController.runningSessionsJson()
    let idx = -1
    for (let i = 0; i < running.length; ++i)
      running[i].runtimeId === prev && (idx = i)
    activeRuntime = (idx >= 0) ? idx : (running.length ? 0 : -1)
    refreshTerminal()
  }

  function refreshTerminal()
  {
    terminalText = activeRuntimeId
      ? AppController.terminalSnapshot(activeRuntimeId)
      : ""
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

  function sendKey(event)
  {
    if (!activeRuntimeId)
      return
    let data = ""
    switch (event.key)
    {
      case Qt.Key_Return:
      case Qt.Key_Enter:     data = "\r"; break
      case Qt.Key_Backspace: data = "\x7f"; break
      case Qt.Key_Tab:       data = "\t"; break
      case Qt.Key_Escape:    data = "\x1b"; break
      case Qt.Key_Up:        data = "\x1b[A"; break
      case Qt.Key_Down:      data = "\x1b[B"; break
      case Qt.Key_Right:     data = "\x1b[C"; break
      case Qt.Key_Left:      data = "\x1b[D"; break
      case Qt.Key_Home:      data = "\x1b[H"; break
      case Qt.Key_End:       data = "\x1b[F"; break
      case Qt.Key_PageUp:    data = "\x1b[5~"; break
      case Qt.Key_PageDown:  data = "\x1b[6~"; break
      case Qt.Key_Delete:    data = "\x1b[3~"; break
      default:
        event.text && (data = event.text)
    }
    data && AppController.writeTerminal(activeRuntimeId, data)
    event.accepted = true
  }

  function estimateTermSize()
  {
    // ~ monospace cell size; good enough until we measure font metrics.
    const cols = Math.max(40, Math.floor(termView.width  / 8.2))
    const rows = Math.max(10, Math.floor(termView.height / 16))
    activeRuntimeId
      && AppController.resizeTerminal(activeRuntimeId, rows, cols)
  }

  onClosing: function (close)
  {
    AppController.hideOnClose() && (
      close.accepted = false,
      root.hide(),
      AppController.hideMainWindow()
    )
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

    function onQuitRequested() {}

    function onConfigChanged()
    {
      root.refreshConfig()
    }

    function onRunningChanged()
    {
      root.refreshRunning()
    }

    function onTerminalUpdated(runtimeId)
    {
      runtimeId === root.activeRuntimeId && root.refreshTerminal()
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
        text:    "MVP 4 — terminals"
        opacity: 0.6
      }

      Item { Layout.fillWidth: true }

      Label
      {
        text:    dirty ? qsTr("Unsaved changes") : qsTr("Saved")
        opacity: dirty ? 1.0 : 0.55
        color:   dirty ? palette.link : palette.windowText
      }
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

        // Flush-adjacent tab chrome (≈0–1px).
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
              text:              modelData.name
              width:             implicitWidth + 8
              onClicked:         activeRuntime = index
              // Close affordance via middle-click / context later; Stop button below.
            }
          }
        }

        Frame
        {
          id:                termFrame
          Layout.fillWidth:  true
          Layout.fillHeight: true
          padding:           0

          background: Rectangle
          {
            color: "#1e1e2e"
          }

          Item
          {
            id:      termView
            anchors.fill: parent
            focus:   true

            ScrollView
            {
              anchors.fill: parent
              clip:         true

              TextArea
              {
                id:               termArea
                readOnly:         true
                wrapMode:         TextEdit.NoWrap
                textFormat:       TextEdit.RichText
                text:             terminalText
                color:            "#cdd6f4"
                selectedTextColor: "#1e1e2e"
                selectionColor:   "#89b4fa"
                background:       null
                font.family:      "monospace"
                font.pixelSize:   13
                persistentSelection: true
                focus:            true

                // Keep view at bottom as output grows.
                onTextChanged:
                {
                  cursorPosition = length
                }

                Keys.onPressed: (event) => sendKey(event)
              }
            }

            onWidthChanged:  estimateTermSize()
            onHeightChanged: estimateTermSize()

            MouseArea
            {
              anchors.fill: parent
              acceptedButtons: Qt.LeftButton
              propagateComposedEvents: true
              onClicked:
              {
                termArea.forceActiveFocus()
                mouse.accepted = false
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
            text:    statusText
            opacity: 0.75
            elide:   Text.ElideRight
            Layout.fillWidth: true
          }
        }
      }

      // 1 — Sessions (configure catalog)
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
                  clearEditor()
                  editName.forceActiveFocus()
                }
              }
              Button
              {
                text:    qsTr("Delete")
                enabled: selectedSession >= 0
                onClicked: AppController.removeSession(sessions[selectedSession].id)
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
                text: qsTr("Elevated (start deferred to MVP 6)")
              }
            }

            Item { Layout.fillHeight: true }

            RowLayout
            {
              Layout.alignment: Qt.AlignRight
              spacing:          8

              Button
              {
                text: qsTr("Apply")
                onClicked:
                {
                  const r = AppController.upsertSessionJson(editorPayload())
                  r.indexOf("ERR:") === 0
                    ? (statusText = r.slice(4))
                    : (editSessionId = r, refreshConfig())
                }
              }
              Button
              {
                text:        qsTr("Start")
                enabled:     editName.text.trim().length > 0
                highlighted: true
                onClicked:
                {
                  const r = AppController.upsertSessionJson(editorPayload())
                  if (r.indexOf("ERR:") === 0)
                  {
                    statusText = r.slice(4)
                    return
                  }
                  editSessionId = r
                  refreshConfig()
                  const result = AppController.startConfiguredSession(r)
                  const bad = result.indexOf("failed") >= 0
                    || result.indexOf("Unknown") >= 0
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
            onToggled: AppController.setHideOnClose(checked)
          }

          CheckBox
          {
            id:   autostartBox
            text: qsTr("Autostart with desktop session (stored now; desktop entry in MVP 7)")
            onToggled: AppController.setAutostart(checked)
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
              onActivated:
              {
                currentIndex >= 0 && model[currentIndex]
                  && AppController.setElevationMode(model[currentIndex].id)
              }
            }
          }

          Label
          {
            Layout.fillWidth: true
            wrapMode:         Text.WordWrap
            opacity:          0.7
            text:             qsTr("Running terminals survive Hide to tray. Quit ends all sessions.")
          }

          Item { Layout.fillHeight: true }
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
        text:        qsTr("Save")
        highlighted: dirty
        onClicked:
        {
          const err = AppController.saveConfig()
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

  Component.onCompleted:
  {
    AppController.startTray()
    AppController.showMainWindow()
    refreshConfig()
    refreshRunning()
    AppController.startAutostartSessions()
    termView.forceActiveFocus()
  }
}
