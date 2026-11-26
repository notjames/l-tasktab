import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import l_tasktabs

ApplicationWindow
{
  id: root

  width:         860
  height:        560
  minimumWidth:  640
  minimumHeight: 420
  visible:       true
  title:         AppController.appName()
  color:         palette.window

  property var    sessions:        []
  property var    elevationModes:  []
  property int    selectedSession: -1
  property bool   dirty:           false
  property string statusText:      ""
  property string editSessionId:   ""

  function refreshFromBackend()
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
      if (elevationModeBox.model[i].id === mode)
      {
        elevationModeBox.currentIndex = i
        break
      }
    }

    selectedSession = (selectedSession >= sessions.length)
      ? (sessions.length ? 0 : -1)
      : selectedSession

    selectedSession >= 0
      ? loadEditor(sessions[selectedSession])
      : clearEditor()
  }

  function loadEditor(s)
  {
    editSessionId          = s.id || ""
    editName.text          = s.name || ""
    editCommand.text       = s.command || ""
    editCwd.text           = s.working_directory || "~"
    editAutostart.checked  = !!s.autostart
    editElevated.checked   = !!s.elevated
  }

  function clearEditor()
  {
    editSessionId          = ""
    editName.text          = ""
    editCommand.text       = ""
    editCwd.text           = "~"
    editAutostart.checked  = false
    editElevated.checked   = false
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

    function onQuitRequested()
    {
      // Process exit is std::process::exit(0) from AppController.requestQuit().
    }

    function onConfigChanged()
    {
      root.refreshFromBackend()
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
        text:    "MVP 2 — configure"
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
    anchors.margins: 12
    spacing:         10

    Label
    {
      Layout.fillWidth: true
      wrapMode:         Text.WordWrap
      opacity:          0.75
      text:             qsTr("Config: %1").arg(AppController.configPath())
    }

    TabBar
    {
      id:               tabs
      Layout.fillWidth: true
      // Application Settings first in the bar; Sessions is default on open.
      currentIndex:     1

      TabButton { text: qsTr("Application Settings") }
      TabButton { text: qsTr("Sessions") }
    }

    StackLayout
    {
      Layout.fillWidth:  true
      Layout.fillHeight: true
      currentIndex:      tabs.currentIndex

      // 0 — Application Settings
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
            text:             qsTr("Session terminals arrive in MVP 3. This screen only manages config.toml.")
          }

          Item { Layout.fillHeight: true }
        }
      }

      // 1 — Sessions (default tab)
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
              id:                sessionList
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
                placeholderText:  qsTr("syswatch")
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
                text: qsTr("Elevated")
              }
            }

            Item { Layout.fillHeight: true }

            Button
            {
              text:             qsTr("Apply session")
              Layout.alignment: Qt.AlignRight
              onClicked:
              {
                const err = AppController.upsertSessionJson(editorPayload())
                err && (statusText = err)
              }
            }
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
        text:             statusText
        opacity:          0.8
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
    refreshFromBackend()
  }
}
