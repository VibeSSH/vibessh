package dev.vibessh.intellij

import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.fileChooser.FileChooser
import com.intellij.openapi.fileChooser.FileChooserDescriptor
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.VirtualFile
import java.io.File

/**
 * Build - Deploy to VibeSSH.
 *
 * Picks the artifact, sends it, and says what happened. The application it
 * goes to is chosen here rather than in Settings, because a project usually
 * has one answer and a person usually has several projects.
 *
 * **Everything that touches the network runs under a progress indicator on a
 * background thread.** A deploy uploads over SFTP to a real server and then
 * restarts it; doing that on the interface thread would freeze the IDE for as
 * long as the server takes to come back.
 */
class DeployAction : AnAction() {

    override fun actionPerformed(event: AnActionEvent) {
        val project = event.project ?: return
        val settings = VibeSshSettings.getInstance()

        if (settings.token.isBlank()) {
            Messages.showErrorDialog(
                project,
                "Najpierw wklej token w Ustawienia - Tools - VibeSSH.",
                "VibeSSH",
            )
            return
        }

        val artifact = chooseArtifact(project) ?: return

        // Asked for before the upload rather than assumed from settings: the
        // wrong application here means a plugin jar landing on a server it
        // was never meant for, and being restarted into.
        runInBackground(project, "Pobieranie listy aplikacji") { indicator ->
            val client = VibeSshClient(settings.baseUrl, settings.token)
            val applications = client.listApplications()
            indicator.checkCanceled()
            if (applications.isEmpty()) {
                notify(project, "VibeSSH nie ma żadnych aplikacji do wdrożenia.", NotificationType.WARNING)
                return@runInBackground
            }

            com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                val chosen = chooseApplication(project, applications, settings.applicationId) ?: return@invokeLater
                settings.applicationId = chosen.id
                upload(project, settings, chosen, artifact)
            }
        }
    }

    private fun chooseArtifact(project: Project): File? {
        val descriptor = FileChooserDescriptor(true, false, true, true, false, false)
            .withTitle("Plik do wysłania")
            .withDescription("Zwykle jar z build/libs")
        val chosen: VirtualFile = FileChooser.chooseFile(descriptor, project, project.guessArtifactDirectory()) ?: return null
        return File(chosen.path)
    }

    private fun chooseApplication(
        project: Project,
        applications: List<VibeSshClient.Application>,
        preselect: String,
    ): VibeSshClient.Application? {
        val labels = applications.map { it.toString() }.toTypedArray()
        val initial = applications.indexOfFirst { it.id == preselect }.coerceAtLeast(0)
        val index = Messages.showChooseDialog(
            project,
            "Gdzie wysłać?",
            "VibeSSH",
            null,
            labels,
            labels[initial],
        )
        return applications.getOrNull(index)
    }

    private fun upload(
        project: Project,
        settings: VibeSshSettings,
        application: VibeSshClient.Application,
        artifact: File,
    ) {
        val remotePath = "${settings.remoteDirectory.trim('/')}/${artifact.name}"
        runInBackground(project, "Wysyłanie ${artifact.name} do ${application.name}") {
            val client = VibeSshClient(settings.baseUrl, settings.token)
            client.deploy(application.id, remotePath, artifact, settings.restartAfterDeploy)
            val what = if (settings.restartAfterDeploy) "wgrano i zrestartowano" else "wgrano"
            notify(project, "$what: $remotePath na ${application.name}", NotificationType.INFORMATION)
        }
    }

    /**
     * Runs `work` with a progress bar, turning anything it throws into a
     * notification rather than an IDE error report - a server that was down
     * is not a bug in this plugin, and the platform's error reporter is for
     * bugs.
     */
    private fun runInBackground(project: Project, title: String, work: (ProgressIndicator) -> Unit) {
        ProgressManager.getInstance().run(object : Task.Backgroundable(project, "VibeSSH: $title", true) {
            override fun run(indicator: ProgressIndicator) {
                try {
                    work(indicator)
                } catch (err: VibeSshClient.VibeSshException) {
                    notify(project, err.message ?: "Nie udało się.", NotificationType.ERROR)
                } catch (err: Exception) {
                    notify(project, "VibeSSH: ${err.message ?: err::class.simpleName}", NotificationType.ERROR)
                }
            }
        })
    }

    private fun notify(project: Project, message: String, type: NotificationType) {
        NotificationGroupManager.getInstance()
            .getNotificationGroup("VibeSSH")
            .createNotification(message, type)
            .notify(project)
    }
}

/** `build/libs` when it exists, so the file chooser opens where the jar is. */
private fun Project.guessArtifactDirectory(): VirtualFile? {
    val base = basePath ?: return null
    val candidates = listOf("build/libs", "target", "build")
    for (candidate in candidates) {
        val file = com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath("$base/$candidate")
        if (file != null && file.isDirectory) return file
    }
    return com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(base)
}
