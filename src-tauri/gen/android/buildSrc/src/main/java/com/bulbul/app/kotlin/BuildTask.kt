import java.io.File
import org.apache.tools.ant.taskdefs.condition.Os
import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.logging.LogLevel
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.TaskAction

open class BuildTask : DefaultTask() {
    @Input
    var rootDirRel: String? = null
    @Input
    var target: String? = null
    @Input
    var release: Boolean? = null

    @TaskAction
    fun assemble() {
        val executable = "npm"
        try {
            runTauriCli(executable)
        } catch (e: Exception) {
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                val fallbacks = listOf(
                    "$executable.cmd",
                    "$executable.exe",
                    "$executable.bat",
                    "pnpm.cmd",
                    "pnpm"
                )
                
                var lastException: Exception = e
                for (fallback in fallbacks) {
                    try {
                        runTauriCli(fallback)
                        return
                    } catch (fallbackException: Exception) {
                        lastException = fallbackException
                    }
                }
                throw lastException
            } else {
                throw e
            }
        }
    }

    fun runTauriCli(executable: String) {
        val rootDirRel = rootDirRel ?: throw GradleException("rootDirRel cannot be null")
        val target = target ?: throw GradleException("target cannot be null")
        val release = release ?: throw GradleException("release cannot be null")
        val baseArgs = listOf("run", "--", "tauri", "android", "android-studio-script")

        project.exec {
            workingDir(File(project.projectDir, rootDirRel))
            if (Os.isFamily(Os.FAMILY_WINDOWS)) {
                executable("cmd.exe")
                val fullArgs = mutableListOf("/c", executable)
                fullArgs.addAll(baseArgs)
                if (project.logger.isEnabled(LogLevel.DEBUG)) {
                    fullArgs.add("-vv")
                } else if (project.logger.isEnabled(LogLevel.INFO)) {
                    fullArgs.add("-v")
                }
                if (release) {
                    fullArgs.add("--release")
                }
                fullArgs.addAll(listOf("--target", target))
                args(fullArgs)
            } else {
                executable(executable)
                val fullArgs = baseArgs.toMutableList()
                if (project.logger.isEnabled(LogLevel.DEBUG)) {
                    fullArgs.add("-vv")
                } else if (project.logger.isEnabled(LogLevel.INFO)) {
                    fullArgs.add("-v")
                }
                if (release) {
                    fullArgs.add("--release")
                }
                fullArgs.addAll(listOf("--target", target))
                args(fullArgs)
            }
        }.assertNormalExitValue()
    }
}
