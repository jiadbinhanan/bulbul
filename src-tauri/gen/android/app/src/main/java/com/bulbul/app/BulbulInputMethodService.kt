// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Romanch Roshan Singh

package com.bulbul.app

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.util.Log
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.Toast
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Backspace
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelStoreOwner
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.lifecycle.setViewTreeViewModelStoreOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import android.view.HapticFeedbackConstants
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.ui.platform.LocalView
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

enum class VoiceKeyboardState {
    IDLE,
    RECORDING,
    PROCESSING
}

enum class DeckMode {
    VOICE,
    TYPING
}

enum class TypingLayoutMode {
    QWERTY,
    BENGALI,
    NUM_SYMBOLS
}

enum class ShiftState {
    OFF,
    ONCE,
    CAPS_LOCK
}

data class ContextModePreset(
    val id: Long,
    val title: String,
    val iconName: String
)

val DEFAULT_CONTEXT_MODES = listOf(
    ContextModePreset(1L, "Direct Smart", "mic"),
    ContextModePreset(2L, "Coding Prompt", "code"),
    ContextModePreset(3L, "Pro Email", "mail"),
    ContextModePreset(4L, "Natural Chat", "message"),
    ContextModePreset(5L, "Translate", "translate")
)

val LANGUAGE_OPTIONS = listOf(
    "auto" to "Auto",
    "bn-IN" to "বাংলা",
    "bn-IN-tech" to "Banglish Tech",
    "en" to "English",
    "hi" to "हिंदी"
)

val BENGALI_PUNCTUATION = listOf(
    "।", "?", "!", ",", "`", "\"", ":", "-"
)

data class LastDictationInfo(
    val recordingId: String,
    val phase1RawText: String,
    var phase2ProcessedText: String,
    var currentActiveText: String,
    val transcriptionTryId: Long,
    var lastInsertedLength: Int
)

/**
 * Advanced voice-first Android System Keyboard (IME) powered by Jetpack Compose
 * and the unified Bulbul Rust Core (Milestone 5 complete).
 */
class BulbulInputMethodService : InputMethodService(),
    LifecycleOwner,
    ViewModelStoreOwner,
    SavedStateRegistryOwner {

    companion object {
        private const val TAG = "BulbulIME"
    }

    private val lifecycleRegistry by lazy { LifecycleRegistry(this) }
    override val lifecycle: Lifecycle get() = lifecycleRegistry

    private val mViewModelStore by lazy { ViewModelStore() }
    override val viewModelStore: ViewModelStore get() = mViewModelStore

    private val savedStateRegistryController by lazy { SavedStateRegistryController.create(this) }
    override val savedStateRegistry: SavedStateRegistry get() = savedStateRegistryController.savedStateRegistry

    private val serviceScope = CoroutineScope(Dispatchers.Main + SupervisorJob())
    private var recorder: AudioRecorder? = null

    // State
    private val keyboardState = mutableStateOf(VoiceKeyboardState.IDLE)
    private val deckModeState = mutableStateOf(DeckMode.VOICE)
    private val audioAmplitudeState = mutableFloatStateOf(0f)
    private val activeContextModeId = mutableLongStateOf(1L)
    private val activeLanguage = mutableStateOf("auto")
    private val statusMessage = mutableStateOf<String?>(null)

    // Milestone 5: Ribbon Overrides
    private val phase1ModeState = mutableStateOf("smart") // "smart" or "verbatim"
    private val vocabEnabledState = mutableStateOf(true)
    private val activeVocabProfileIdState = mutableStateOf<Long?>(null)
    private val phase2OverrideState = mutableStateOf<String?>("auto") // "auto", "off", "gemini-flash", "gemini-pro", "groq"

    // Milestone 5: Selection Action State
    private val selectedTextState = mutableStateOf("")

    // Milestone 5: Post-Dictation Contextual Magic Bar
    private val lastDictationState = mutableStateOf<LastDictationInfo?>(null)

    // Milestone 5: Mini History Drawer
    private val showHistoryDrawerState = mutableStateOf(false)
    private val recentRecordingsState = mutableStateOf<List<HistoryItem>>(emptyList())
    private val vocabProfilesState = mutableStateOf<List<VocabProfileItem>>(emptyList())
    private val contextModesState = mutableStateOf<List<ContextModeItem>>(emptyList())

    override fun onCreate() {
        super.onCreate()
        BulbulBridge.ensureInitialized(this)
        savedStateRegistryController.performRestore(null)
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_CREATE)
        recorder = AudioRecorder(this)
        activeLanguage.value = BulbulConfig.language(this)
        activeContextModeId.longValue = BulbulConfig.activeContextModeId(this)
        loadDataFromRust()
        Log.i(TAG, "BulbulInputMethodService created")
    }

    override fun onWindowShown() {
        super.onWindowShown()
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_START)
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_RESUME)
        activeLanguage.value = BulbulConfig.language(this)
        activeContextModeId.longValue = BulbulConfig.activeContextModeId(this)
        updateSelectedText()
        loadDataFromRust()
    }

    override fun onWindowHidden() {
        super.onWindowHidden()
        if (keyboardState.value == VoiceKeyboardState.RECORDING) {
            cancelRecording()
        }
        showHistoryDrawerState.value = false
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_PAUSE)
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_STOP)
    }

    override fun onDestroy() {
        super.onDestroy()
        cancelRecording()
        recorder = null
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_DESTROY)
        mViewModelStore.clear()
        Log.i(TAG, "BulbulInputMethodService destroyed")
    }

    override fun onUpdateSelection(
        oldSelStart: Int,
        oldSelEnd: Int,
        newSelStart: Int,
        newSelEnd: Int,
        candidatesStart: Int,
        candidatesEnd: Int
    ) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        updateSelectedText()
    }

    private fun updateSelectedText() {
        try {
            val ic = currentInputConnection
            val sel = ic?.getSelectedText(0)?.toString() ?: ""
            selectedTextState.value = sel
        } catch (_: Throwable) {
            selectedTextState.value = ""
        }
    }

    private fun loadDataFromRust() {
        serviceScope.launch(Dispatchers.IO) {
            try {
                val modes = BulbulBridge.fetchContextModes()
                if (modes.isNotEmpty()) {
                    withContext(Dispatchers.Main) {
                        contextModesState.value = modes
                    }
                }
                val vocabs = BulbulBridge.fetchVocabularyProfiles()
                withContext(Dispatchers.Main) {
                    vocabProfilesState.value = vocabs
                }
                val recs = BulbulBridge.fetchRecentRecordings(20)
                withContext(Dispatchers.Main) {
                    recentRecordingsState.value = recs
                }
            } catch (t: Throwable) {
                Log.w(TAG, "loadDataFromRust non-fatal", t)
            }
        }
    }

    override fun onCreateInputView(): View {
        window?.window?.decorView?.let { decor ->
            decor.setViewTreeLifecycleOwner(this)
            decor.setViewTreeViewModelStoreOwner(this)
            decor.setViewTreeSavedStateRegistryOwner(this)
        }

        return ComposeView(this).apply {
            setViewTreeLifecycleOwner(this@BulbulInputMethodService)
            setViewTreeViewModelStoreOwner(this@BulbulInputMethodService)
            setViewTreeSavedStateRegistryOwner(this@BulbulInputMethodService)

            setContent {
                BulbulKeyboardTheme {
                    BulbulKeyboardScreen(
                        state = keyboardState.value,
                        deckMode = deckModeState.value,
                        audioAmplitude = audioAmplitudeState.floatValue,
                        contextModeId = activeContextModeId.longValue,
                        language = activeLanguage.value,
                        phase1Mode = phase1ModeState.value,
                        vocabEnabled = vocabEnabledState.value,
                        activeVocabProfileId = activeVocabProfileIdState.value,
                        phase2Override = phase2OverrideState.value,
                        statusMessage = statusMessage.value,
                        selectedText = selectedTextState.value,
                        lastDictation = lastDictationState.value,
                        showHistoryDrawer = showHistoryDrawerState.value,
                        recentRecordings = recentRecordingsState.value,
                        vocabProfiles = vocabProfilesState.value,
                        contextModes = contextModesState.value,
                        onMicTap = { handleMicToggle() },
                        onToggleDeckMode = {
                            deckModeState.value = if (deckModeState.value == DeckMode.VOICE) DeckMode.TYPING else DeckMode.VOICE
                        },
                        onCharTap = { char ->
                            currentInputConnection?.commitText(char, 1)
                        },
                        onContextModeSelected = { modeId ->
                            activeContextModeId.longValue = modeId
                        },
                        onLanguageSelected = { lang ->
                            activeLanguage.value = lang
                        },
                        onPhase1ModeToggle = {
                            phase1ModeState.value = if (phase1ModeState.value == "smart") "verbatim" else "smart"
                        },
                        onVocabToggle = {
                            vocabEnabledState.value = !vocabEnabledState.value
                        },
                        onVocabProfileSelected = { profileId ->
                            activeVocabProfileIdState.value = profileId
                        },
                        onPhase2OverrideSelected = { overrideVal ->
                            phase2OverrideState.value = overrideVal
                        },
                        onToggleHistoryDrawer = {
                            showHistoryDrawerState.value = !showHistoryDrawerState.value
                            if (showHistoryDrawerState.value) {
                                loadDataFromRust()
                            }
                        },
                        onHistoryItemClicked = { itemText ->
                            currentInputConnection?.commitText(itemText, 1)
                            showHistoryDrawerState.value = false
                        },
                        onTransformSelection = { action ->
                            handleTransformSelection(action)
                        },
                        onSwitchMagicBar = {
                            handleSwitchMagicBar()
                        },
                        onRetryMagicBar = { modeId, prov, model ->
                            handleRetryMagicBar(modeId, prov, model)
                        },
                        onDismissMagicBar = {
                            lastDictationState.value = null
                        },
                        onPunctuationTap = { punct ->
                            currentInputConnection?.commitText(punct, 1)
                        },
                        onMacroAction = { action ->
                            handleMacroAction(action)
                        },
                        onSpace = { handleSpace() },
                        onBackspace = { handleBackspace() },
                        onAction = { handleAction() },
                        onCursorMoveLeft = { handleCursorMoveLeft() },
                        onCursorMoveRight = { handleCursorMoveRight() },
                        onSwitchKeyboard = { handleSwitchKeyboard() },
                        onOpenSettings = { handleOpenSettings() }
                    )
                }
            }
        }
    }

    private fun handleMicToggle() {
        when (keyboardState.value) {
            VoiceKeyboardState.IDLE -> startRecording()
            VoiceKeyboardState.RECORDING -> stopRecordingAndProcess()
            VoiceKeyboardState.PROCESSING -> {}
        }
    }

    private fun startRecording() {
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED) {
            Toast.makeText(this, "Microphone permission required for Bulbul", Toast.LENGTH_SHORT).show()
            val intent = Intent(this, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK
            }
            startActivity(intent)
            return
        }

        val r = recorder ?: return
        val started = r.start()
        if (started) {
            keyboardState.value = VoiceKeyboardState.RECORDING
            val sel = selectedTextState.value
            statusMessage.value = if (sel.isNotBlank()) "Speak command for selection..." else "Listening..."
            serviceScope.launch {
                while (keyboardState.value == VoiceKeyboardState.RECORDING) {
                    audioAmplitudeState.floatValue = recorder?.amplitude ?: 0f
                    delay(50)
                }
                audioAmplitudeState.floatValue = 0f
            }
        } else {
            statusMessage.value = "Could not start mic"
            Toast.makeText(this, "Microphone unavailable", Toast.LENGTH_SHORT).show()
        }
    }

    private fun stopRecordingAndProcess() {
        val r = recorder ?: return
        keyboardState.value = VoiceKeyboardState.PROCESSING
        audioAmplitudeState.floatValue = 0f
        statusMessage.value = "Polishing with AI..."

        val selected = selectedTextState.value
        val lang = activeLanguage.value
        val modeId = activeContextModeId.longValue
        val p1 = phase1ModeState.value
        val vEnabled = vocabEnabledState.value
        val vId = activeVocabProfileIdState.value

        // Resolve Phase 2 overrides
        var p2Enabled: Boolean? = null
        var p2Prov: String? = null
        var p2Model: String? = null
        when (phase2OverrideState.value) {
            "off" -> p2Enabled = false
            "gemini-flash" -> {
                p2Enabled = true
                p2Prov = "gemini"
                p2Model = "gemini-2.5-flash"
            }
            "gemini-pro" -> {
                p2Enabled = true
                p2Prov = "gemini"
                p2Model = "gemini-2.5-pro"
            }
            "groq" -> {
                p2Enabled = true
                p2Prov = "groq"
                p2Model = "llama-3.3-70b-versatile"
            }
            else -> {}
        }

        val options = ProcessAudioOptions(
            language = lang,
            contextModeId = modeId,
            phase1Mode = p1,
            vocabEnabled = vEnabled,
            vocabProfileId = vId,
            phase2Enabled = p2Enabled,
            phase2Provider = p2Prov,
            phase2Model = p2Model,
            selectedText = if (selected.isNotBlank()) selected else null
        )

        serviceScope.launch {
            val pcm = withContext(Dispatchers.IO) {
                r.stopRawPcm()
            }

            if (pcm == null || pcm.isEmpty()) {
                keyboardState.value = VoiceKeyboardState.IDLE
                statusMessage.value = "No audio detected"
                return@launch
            }

            val resJson = withContext(Dispatchers.IO) {
                try {
                    BulbulBridge.processVoiceAudioWithOptions(
                        pcm,
                        AudioRecorder.SAMPLE_RATE_HZ,
                        options.toJson()
                    )
                } catch (t: Throwable) {
                    Log.e(TAG, "processVoiceAudioWithOptions failed", t)
                    "{}"
                }
            }

            val result = ProcessAudioResult.fromJson(resJson)

            if (result.activeText.isNotBlank()) {
                currentInputConnection?.commitText(result.activeText, 1)
                statusMessage.value = null

                // Store for contextual magic bar
                lastDictationState.value = LastDictationInfo(
                    recordingId = result.recordingId,
                    phase1RawText = result.phase1RawText,
                    phase2ProcessedText = result.phase2ProcessedText,
                    currentActiveText = result.activeText,
                    transcriptionTryId = result.transcriptionTryId,
                    lastInsertedLength = result.activeText.length
                )
            } else {
                statusMessage.value = "No speech detected (or check API keys)"
                Toast.makeText(this@BulbulInputMethodService, "Bulbul: কোনো কথা বোঝা যায়নি বা API Key চেক করুন", Toast.LENGTH_SHORT).show()
            }

            keyboardState.value = VoiceKeyboardState.IDLE
            updateSelectedText()
        }
    }

    private fun handleTransformSelection(action: String) {
        val selected = selectedTextState.value
        if (selected.isBlank()) return

        keyboardState.value = VoiceKeyboardState.PROCESSING
        statusMessage.value = "Transforming with AI..."

        serviceScope.launch {
            val transformed = withContext(Dispatchers.IO) {
                try {
                    BulbulBridge.transformSelectedText(selected, action, null)
                } catch (t: Throwable) {
                    Log.e(TAG, "transformSelectedText failed", t)
                    selected
                }
            }

            if (transformed.isNotBlank()) {
                currentInputConnection?.commitText(transformed, 1)
                statusMessage.value = null
            } else {
                statusMessage.value = "Transform failed"
            }

            keyboardState.value = VoiceKeyboardState.IDLE
            updateSelectedText()
        }
    }

    private fun handleSwitchMagicBar() {
        val last = lastDictationState.value ?: return
        val ic = currentInputConnection ?: return

        val nextText = if (last.currentActiveText == last.phase2ProcessedText && last.phase1RawText.isNotBlank()) {
            last.phase1RawText
        } else if (last.phase2ProcessedText.isNotBlank()) {
            last.phase2ProcessedText
        } else {
            return
        }

        ic.deleteSurroundingText(last.lastInsertedLength, 0)
        ic.commitText(nextText, 1)

        last.currentActiveText = nextText
        last.lastInsertedLength = nextText.length
        lastDictationState.value = last.copy()
    }

    private fun handleRetryMagicBar(modeId: Long, provider: String?, model: String?) {
        val last = lastDictationState.value ?: return
        val ic = currentInputConnection ?: return

        keyboardState.value = VoiceKeyboardState.PROCESSING
        statusMessage.value = "Re-trying with $model..."

        serviceScope.launch {
            val newText = withContext(Dispatchers.IO) {
                try {
                    BulbulBridge.rerunPhase2(
                        last.recordingId,
                        last.transcriptionTryId,
                        modeId,
                        provider,
                        model,
                        null
                    )
                } catch (t: Throwable) {
                    Log.e(TAG, "rerunPhase2 failed", t)
                    ""
                }
            }

            if (newText.isNotBlank()) {
                ic.deleteSurroundingText(last.lastInsertedLength, 0)
                ic.commitText(newText, 1)

                last.currentActiveText = newText
                last.phase2ProcessedText = newText
                last.lastInsertedLength = newText.length
                lastDictationState.value = last.copy()
                statusMessage.value = null
            } else {
                statusMessage.value = "Re-try failed"
            }

            keyboardState.value = VoiceKeyboardState.IDLE
        }
    }

    private fun handleMacroAction(action: String) {
        val ic = currentInputConnection ?: return
        when (action) {
            "Undo" -> ic.performContextMenuAction(android.R.id.undo)
            "Redo" -> ic.performContextMenuAction(android.R.id.redo)
            "Cut" -> ic.performContextMenuAction(android.R.id.cut)
            "Copy" -> ic.performContextMenuAction(android.R.id.copy)
            "Paste" -> ic.performContextMenuAction(android.R.id.paste)
            "Select All" -> ic.performContextMenuAction(android.R.id.selectAll)
            "Del Word" -> handleDeleteWord()
        }
    }

    private fun handleDeleteWord() {
        val ic = currentInputConnection ?: return
        val selected = ic.getSelectedText(0)
        if (!selected.isNullOrEmpty()) {
            ic.commitText("", 1)
            return
        }
        val before = ic.getTextBeforeCursor(50, 0)?.toString() ?: ""
        if (before.isEmpty()) {
            ic.deleteSurroundingText(1, 0)
            return
        }
        var trimEnd = before.length
        while (trimEnd > 0 && before[trimEnd - 1].isWhitespace()) {
            trimEnd--
        }
        while (trimEnd > 0 && !before[trimEnd - 1].isWhitespace()) {
            trimEnd--
        }
        val toDelete = before.length - trimEnd
        if (toDelete > 0) {
            ic.deleteSurroundingText(toDelete, 0)
        } else {
            ic.deleteSurroundingText(1, 0)
        }
    }

    private fun handleCursorMoveLeft() {
        val ic = currentInputConnection ?: return
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_LEFT))
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DPAD_LEFT))
    }

    private fun handleCursorMoveRight() {
        val ic = currentInputConnection ?: return
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_RIGHT))
        ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DPAD_RIGHT))
    }

    private fun cancelRecording() {
        try {
            recorder?.stopRawPcm()
        } catch (_: Throwable) {}
        keyboardState.value = VoiceKeyboardState.IDLE
        audioAmplitudeState.floatValue = 0f
        statusMessage.value = null
    }

    private fun handleSpace() {
        currentInputConnection?.commitText(" ", 1)
    }

    private fun handleBackspace() {
        val ic = currentInputConnection ?: return
        val selected = ic.getSelectedText(0)
        if (selected.isNullOrEmpty()) {
            ic.deleteSurroundingText(1, 0)
        } else {
            ic.commitText("", 1)
        }
    }

    private fun handleAction() {
        val ic = currentInputConnection ?: return
        val info = currentInputEditorInfo
        val actionId = info.imeOptions and EditorInfo.IME_MASK_ACTION
        if (actionId != EditorInfo.IME_ACTION_NONE && actionId != EditorInfo.IME_ACTION_UNSPECIFIED) {
            ic.performEditorAction(actionId)
        } else {
            ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER))
            ic.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER))
        }
    }

    private fun handleSwitchKeyboard() {
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
        val windowToken = window?.window?.attributes?.token

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            if (!switchToPreviousInputMethod()) {
                if (windowToken != null && imm != null) {
                    imm.switchToNextInputMethod(windowToken, false)
                } else {
                    imm?.showInputMethodPicker()
                }
            }
        } else {
            if (windowToken != null && imm != null) {
                imm.switchToNextInputMethod(windowToken, false)
            } else {
                imm?.showInputMethodPicker()
            }
        }
    }

    private fun handleOpenSettings() {
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK
        }
        startActivity(intent)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Compose Keyboard UI
// ─────────────────────────────────────────────────────────────────────────────

@Composable
fun BulbulKeyboardTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val colorScheme = if (dark) {
        darkColorScheme(
            primary = Color(0xFF6366F1),
            onPrimary = Color.White,
            primaryContainer = Color(0xFF312E81),
            onPrimaryContainer = Color(0xFFE0E7FF),
            surface = Color(0xFF0F172A),
            onSurface = Color(0xFFF8FAFC),
            surfaceVariant = Color(0xFF1E293B),
            onSurfaceVariant = Color(0xFF94A3B8),
            error = Color(0xFFEF4444)
        )
    } else {
        lightColorScheme(
            primary = Color(0xFF4F46E5),
            onPrimary = Color.White,
            primaryContainer = Color(0xFFEEF2FF),
            onPrimaryContainer = Color(0xFF312E81),
            surface = Color(0xFFFFFFFF),
            onSurface = Color(0xFF0F172A),
            surfaceVariant = Color(0xFFF1F5F9),
            onSurfaceVariant = Color(0xFF64748B),
            error = Color(0xFFDC2626)
        )
    }

    MaterialTheme(colorScheme = colorScheme, content = content)
}

@Composable
fun BulbulKeyboardScreen(
    state: VoiceKeyboardState,
    deckMode: DeckMode,
    audioAmplitude: Float,
    contextModeId: Long,
    language: String,
    phase1Mode: String,
    vocabEnabled: Boolean,
    activeVocabProfileId: Long?,
    phase2Override: String?,
    statusMessage: String?,
    selectedText: String,
    lastDictation: LastDictationInfo?,
    showHistoryDrawer: Boolean,
    recentRecordings: List<HistoryItem>,
    vocabProfiles: List<VocabProfileItem>,
    contextModes: List<ContextModeItem>,
    onMicTap: () -> Unit,
    onToggleDeckMode: () -> Unit,
    onCharTap: (String) -> Unit,
    onContextModeSelected: (Long) -> Unit,
    onLanguageSelected: (String) -> Unit,
    onPhase1ModeToggle: () -> Unit,
    onVocabToggle: () -> Unit,
    onVocabProfileSelected: (Long?) -> Unit,
    onPhase2OverrideSelected: (String?) -> Unit,
    onToggleHistoryDrawer: () -> Unit,
    onHistoryItemClicked: (String) -> Unit,
    onTransformSelection: (String) -> Unit,
    onSwitchMagicBar: () -> Unit,
    onRetryMagicBar: (Long, String?, String?) -> Unit,
    onDismissMagicBar: () -> Unit,
    onPunctuationTap: (String) -> Unit,
    onMacroAction: (String) -> Unit,
    onSpace: () -> Unit,
    onBackspace: () -> Unit,
    onAction: () -> Unit,
    onCursorMoveLeft: () -> Unit,
    onCursorMoveRight: () -> Unit,
    onSwitchKeyboard: () -> Unit,
    onOpenSettings: () -> Unit
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding(),
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 6.dp
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 8.dp, vertical = 5.dp),
            verticalArrangement = Arrangement.Top,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            // Milestone 5 + FlorisBoard: Full Live Control Ribbon with Deck Switcher
            LiveControlRibbon(
                deckMode = deckMode,
                onToggleDeckMode = onToggleDeckMode,
                contextModeId = contextModeId,
                language = language,
                phase1Mode = phase1Mode,
                vocabEnabled = vocabEnabled,
                activeVocabProfileId = activeVocabProfileId,
                phase2Override = phase2Override,
                vocabProfiles = vocabProfiles,
                contextModes = contextModes,
                onContextModeSelected = onContextModeSelected,
                onLanguageSelected = onLanguageSelected,
                onPhase1ModeToggle = onPhase1ModeToggle,
                onVocabToggle = onVocabToggle,
                onVocabProfileSelected = onVocabProfileSelected,
                onPhase2OverrideSelected = onPhase2OverrideSelected,
                onToggleHistoryDrawer = onToggleHistoryDrawer
            )

            if (showHistoryDrawer) {
                // Mini History Drawer View
                MiniHistoryDrawer(
                    recordings = recentRecordings,
                    onItemClick = onHistoryItemClicked,
                    onClose = onToggleHistoryDrawer
                )
            } else if (deckMode == DeckMode.VOICE) {
                // Deck A: Voice Dictation & AI Command Deck
                if (selectedText.isNotBlank()) {
                    SelectionActionBar(
                        selectedText = selectedText,
                        onTransform = onTransformSelection
                    )
                }

                Spacer(modifier = Modifier.height(4.dp))

                // Center Dictation Hero Button with live reactive audio waveform ripple
                VoiceHeroButton(
                    state = state,
                    statusMessage = statusMessage,
                    audioAmplitude = audioAmplitude,
                    onMicTap = onMicTap
                )

                // Milestone 5: Post-Dictation Contextual Magic Bar
                if (lastDictation != null) {
                    PostDictationMagicBar(
                        last = lastDictation,
                        contextModes = contextModes,
                        onSwitch = onSwitchMagicBar,
                        onRetry = onRetryMagicBar,
                        onDismiss = onDismissMagicBar
                    )
                }

                // Macro Editing Row
                MacroEditingRow(onMacroAction = onMacroAction)

                // Bilingual Punctuation Row
                BilingualPunctuationRow(onPunctuationTap = onPunctuationTap)

                Spacer(modifier = Modifier.height(4.dp))

                // Bottom Utilities Bar with Swipe Spacebar & Keyboard Switch
                KeyboardBottomBar(
                    onSpace = onSpace,
                    onBackspace = onBackspace,
                    onAction = onAction,
                    onCursorMoveLeft = onCursorMoveLeft,
                    onCursorMoveRight = onCursorMoveRight,
                    onSwitchKeyboard = onSwitchKeyboard,
                    onOpenSettings = onOpenSettings,
                    onSwitchToTyping = onToggleDeckMode
                )
            } else {
                // Deck B: FlorisBoard-Inspired Full Virtual Typing Deck (QWERTY / Bengali / NumSymbols)
                TypingDeckView(
                    onCharTap = onCharTap,
                    onSpace = onSpace,
                    onBackspace = onBackspace,
                    onAction = onAction,
                    onCursorMoveLeft = onCursorMoveLeft,
                    onCursorMoveRight = onCursorMoveRight,
                    onSwitchKeyboard = onSwitchKeyboard,
                    onVoiceTap = {
                        onToggleDeckMode()
                        if (state == VoiceKeyboardState.IDLE) {
                            onMicTap()
                        }
                    }
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun LiveControlRibbon(
    deckMode: DeckMode,
    onToggleDeckMode: () -> Unit,
    contextModeId: Long,
    language: String,
    phase1Mode: String,
    vocabEnabled: Boolean,
    activeVocabProfileId: Long?,
    phase2Override: String?,
    vocabProfiles: List<VocabProfileItem>,
    contextModes: List<ContextModeItem>,
    onContextModeSelected: (Long) -> Unit,
    onLanguageSelected: (String) -> Unit,
    onPhase1ModeToggle: () -> Unit,
    onVocabToggle: () -> Unit,
    onVocabProfileSelected: (Long?) -> Unit,
    onPhase2OverrideSelected: (String?) -> Unit,
    onToggleHistoryDrawer: () -> Unit
) {
    var modeMenuExpanded by remember { mutableStateOf(false) }
    var langMenuExpanded by remember { mutableStateOf(false) }
    var vocabMenuExpanded by remember { mutableStateOf(false) }
    var p2MenuExpanded by remember { mutableStateOf(false) }

    val activeModeName = contextModes.find { it.id == contextModeId }?.name
        ?: DEFAULT_CONTEXT_MODES.find { it.id == contextModeId }?.title
        ?: "Direct Smart"
    val activeLangLabel = LANGUAGE_OPTIONS.find { it.first == language }?.second ?: "Auto"

    val activeVocabName = if (!vocabEnabled) {
        "Vocab: OFF"
    } else if (activeVocabProfileId != null) {
        val p = vocabProfiles.find { it.id == activeVocabProfileId }
        if (p != null) "Vocab: ${p.name.take(8)}" else "Vocab: ON"
    } else {
        "Vocab: ON"
    }

    val p2Label = when (phase2Override) {
        "off" -> "P2: OFF"
        "gemini-flash" -> "P2: Flash"
        "gemini-pro" -> "P2: Pro"
        "groq" -> "P2: Groq"
        else -> "P2: Auto"
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        // 0. Deck Switcher Pill (Voice vs Typing)
        FilterChip(
            selected = deckMode == DeckMode.TYPING,
            onClick = onToggleDeckMode,
            label = {
                Text(
                    if (deckMode == DeckMode.VOICE) "⌨️ Type" else "🎙️ Voice",
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold
                )
            },
            colors = FilterChipDefaults.filterChipColors(
                selectedContainerColor = MaterialTheme.colorScheme.primary,
                selectedLabelColor = Color.White,
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                labelColor = MaterialTheme.colorScheme.primary
            ),
            border = null,
            shape = RoundedCornerShape(14.dp)
        )

        // 1. Context Mode Chip
        Box {
            FilterChip(
                selected = true,
                onClick = { modeMenuExpanded = true },
                label = { Text(activeModeName, fontSize = 11.sp, fontWeight = FontWeight.SemiBold) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = MaterialTheme.colorScheme.primaryContainer,
                    selectedLabelColor = MaterialTheme.colorScheme.onPrimaryContainer
                ),
                border = null,
                shape = RoundedCornerShape(14.dp)
            )

            DropdownMenu(
                expanded = modeMenuExpanded,
                onDismissRequest = { modeMenuExpanded = false }
            ) {
                if (contextModes.isNotEmpty()) {
                    contextModes.forEach { mode ->
                        DropdownMenuItem(
                            text = { Text(mode.name, fontWeight = if (mode.id == contextModeId) FontWeight.Bold else FontWeight.Normal) },
                            onClick = {
                                onContextModeSelected(mode.id)
                                modeMenuExpanded = false
                            },
                            trailingIcon = if (mode.id == contextModeId) {
                                { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                            } else null
                        )
                    }
                } else {
                    DEFAULT_CONTEXT_MODES.forEach { mode ->
                        DropdownMenuItem(
                            text = { Text(mode.title, fontWeight = if (mode.id == contextModeId) FontWeight.Bold else FontWeight.Normal) },
                            onClick = {
                                onContextModeSelected(mode.id)
                                modeMenuExpanded = false
                            },
                            trailingIcon = if (mode.id == contextModeId) {
                                { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                            } else null
                        )
                    }
                }
            }
        }

        // 2. Phase 1 Mode Toggle (SMART vs VERBATIM)
        FilterChip(
            selected = phase1Mode == "smart",
            onClick = onPhase1ModeToggle,
            label = {
                Text(
                    if (phase1Mode == "smart") "P1: SMART" else "P1: VERBATIM",
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Medium
                )
            },
            colors = FilterChipDefaults.filterChipColors(
                selectedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                selectedLabelColor = MaterialTheme.colorScheme.onSurfaceVariant
            ),
            border = null,
            shape = RoundedCornerShape(14.dp)
        )

        // 3. Vocabulary Selector Chip
        Box {
            FilterChip(
                selected = vocabEnabled,
                onClick = { vocabMenuExpanded = true },
                label = { Text(activeVocabName, fontSize = 11.sp, fontWeight = FontWeight.Medium) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = if (vocabEnabled) MaterialTheme.colorScheme.surfaceVariant else MaterialTheme.colorScheme.error.copy(alpha = 0.15f),
                    selectedLabelColor = if (vocabEnabled) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.error
                ),
                border = null,
                shape = RoundedCornerShape(14.dp)
            )

            DropdownMenu(
                expanded = vocabMenuExpanded,
                onDismissRequest = { vocabMenuExpanded = false }
            ) {
                DropdownMenuItem(
                    text = { Text(if (vocabEnabled) "Disable Vocabulary" else "Enable Vocabulary", fontWeight = FontWeight.Bold) },
                    onClick = {
                        onVocabToggle()
                        vocabMenuExpanded = false
                    }
                )
                DropdownMenuItem(
                    text = { Text("All Active Profiles") },
                    onClick = {
                        onVocabProfileSelected(null)
                        vocabMenuExpanded = false
                    },
                    trailingIcon = if (activeVocabProfileId == null) {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
                vocabProfiles.forEach { profile ->
                    DropdownMenuItem(
                        text = { Text("${profile.name} (${profile.termCount})") },
                        onClick = {
                            onVocabProfileSelected(profile.id)
                            vocabMenuExpanded = false
                        },
                        trailingIcon = if (activeVocabProfileId == profile.id) {
                            { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                        } else null
                    )
                }
            }
        }

        // 4. Phase 2 Override Chip
        Box {
            FilterChip(
                selected = phase2Override != "off",
                onClick = { p2MenuExpanded = true },
                label = { Text(p2Label, fontSize = 11.sp, fontWeight = FontWeight.Medium) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                    selectedLabelColor = MaterialTheme.colorScheme.onSurfaceVariant
                ),
                border = null,
                shape = RoundedCornerShape(14.dp)
            )

            DropdownMenu(
                expanded = p2MenuExpanded,
                onDismissRequest = { p2MenuExpanded = false }
            ) {
                DropdownMenuItem(
                    text = { Text("Auto (Mode Default)") },
                    onClick = {
                        onPhase2OverrideSelected("auto")
                        p2MenuExpanded = false
                    },
                    trailingIcon = if (phase2Override == "auto") {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
                DropdownMenuItem(
                    text = { Text("Turn Phase 2 OFF (Raw Only)") },
                    onClick = {
                        onPhase2OverrideSelected("off")
                        p2MenuExpanded = false
                    },
                    trailingIcon = if (phase2Override == "off") {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
                DropdownMenuItem(
                    text = { Text("Gemini 2.5 Flash") },
                    onClick = {
                        onPhase2OverrideSelected("gemini-flash")
                        p2MenuExpanded = false
                    },
                    trailingIcon = if (phase2Override == "gemini-flash") {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
                DropdownMenuItem(
                    text = { Text("Gemini 2.5 Pro") },
                    onClick = {
                        onPhase2OverrideSelected("gemini-pro")
                        p2MenuExpanded = false
                    },
                    trailingIcon = if (phase2Override == "gemini-pro") {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
                DropdownMenuItem(
                    text = { Text("Groq Llama 3.3") },
                    onClick = {
                        onPhase2OverrideSelected("groq")
                        p2MenuExpanded = false
                    },
                    trailingIcon = if (phase2Override == "groq") {
                        { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                    } else null
                )
            }
        }

        // 5. Language Selector Pill
        Box {
            FilterChip(
                selected = false,
                onClick = { langMenuExpanded = true },
                label = { Text(activeLangLabel, fontSize = 11.sp, fontWeight = FontWeight.SemiBold) },
                colors = FilterChipDefaults.filterChipColors(
                    containerColor = MaterialTheme.colorScheme.surfaceVariant,
                    labelColor = MaterialTheme.colorScheme.onSurfaceVariant
                ),
                border = null,
                shape = RoundedCornerShape(14.dp)
            )

            DropdownMenu(
                expanded = langMenuExpanded,
                onDismissRequest = { langMenuExpanded = false }
            ) {
                LANGUAGE_OPTIONS.forEach { (code, label) ->
                    DropdownMenuItem(
                        text = { Text(label, fontWeight = if (code == language) FontWeight.Bold else FontWeight.Normal) },
                        onClick = {
                            onLanguageSelected(code)
                            langMenuExpanded = false
                        },
                        trailingIcon = if (code == language) {
                            { Icon(Icons.Default.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) }
                        } else null
                    )
                }
            }
        }

        // 6. Mini History Drawer Toggle Button
        FilterChip(
            selected = false,
            onClick = onToggleHistoryDrawer,
            label = { Text("🕒 History", fontSize = 11.sp, fontWeight = FontWeight.Medium) },
            colors = FilterChipDefaults.filterChipColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                labelColor = MaterialTheme.colorScheme.onSurfaceVariant
            ),
            border = null,
            shape = RoundedCornerShape(14.dp)
        )
    }
}

@Composable
private fun SelectionActionBar(
    selectedText: String,
    onTransform: (String) -> Unit
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.5f),
        shape = RoundedCornerShape(12.dp)
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp)
        ) {
            Text(
                text = "Selection: \"${selectedText.take(24)}${if (selectedText.length > 24) "…" else ""}\"",
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )

            Spacer(modifier = Modifier.height(4.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                listOf(
                    "Polish" to "✨ Polish",
                    "Translate" to "🌐 Translate",
                    "Code" to "💻 Code",
                    "Email" to "✉️ Email"
                ).forEach { (actionKey, label) ->
                    Surface(
                        modifier = Modifier
                            .weight(1f)
                            .height(28.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { onTransform(actionKey) },
                        color = MaterialTheme.colorScheme.primary,
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(
                                text = label,
                                fontSize = 11.sp,
                                fontWeight = FontWeight.Bold,
                                color = Color.White
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun PostDictationMagicBar(
    last: LastDictationInfo,
    contextModes: List<ContextModeItem>,
    onSwitch: () -> Unit,
    onRetry: (Long, String?, String?) -> Unit,
    onDismiss: () -> Unit
) {
    var retryMenuExpanded by remember { mutableStateOf(false) }

    val isCurrentProcessed = last.currentActiveText == last.phase2ProcessedText
    val switchLabel = if (isCurrentProcessed) "⇄ Switch to Raw" else "⇄ Switch to Polished"

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(12.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            // [⇄ Switch: Raw <-> Processed]
            Surface(
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable { onSwitch() },
                color = MaterialTheme.colorScheme.primaryContainer,
                shape = RoundedCornerShape(8.dp)
            ) {
                Text(
                    text = switchLabel,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                    modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp)
                )
            }

            // [↻ Re-Try]
            Box {
                Surface(
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .clickable { retryMenuExpanded = true },
                    color = MaterialTheme.colorScheme.primaryContainer,
                    shape = RoundedCornerShape(8.dp)
                ) {
                    Text(
                        text = "↻ Re-Try",
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp)
                    )
                }

                DropdownMenu(
                    expanded = retryMenuExpanded,
                    onDismissRequest = { retryMenuExpanded = false }
                ) {
                    DropdownMenuItem(
                        text = { Text("Re-Try: Coding Prompt") },
                        onClick = {
                            val mode = contextModes.find { it.name.lowercase().contains("code") }
                            onRetry(mode?.id ?: 2L, "gemini", "gemini-2.5-flash")
                            retryMenuExpanded = false
                        }
                    )
                    DropdownMenuItem(
                        text = { Text("Re-Try: Pro Email") },
                        onClick = {
                            val mode = contextModes.find { it.name.lowercase().contains("email") || it.name.lowercase().contains("mail") }
                            onRetry(mode?.id ?: 3L, "gemini", "gemini-2.5-flash")
                            retryMenuExpanded = false
                        }
                    )
                    DropdownMenuItem(
                        text = { Text("Re-Try: Gemini 2.5 Pro (Deep)") },
                        onClick = {
                            onRetry(1L, "gemini", "gemini-2.5-pro")
                            retryMenuExpanded = false
                        }
                    )
                    DropdownMenuItem(
                        text = { Text("Re-Try: Groq Llama 3.3") },
                        onClick = {
                            onRetry(1L, "groq", "llama-3.3-70b-versatile")
                            retryMenuExpanded = false
                        }
                    )
                }
            }

            // Dismiss Button
            IconButton(
                onClick = onDismiss,
                modifier = Modifier.size(28.dp)
            ) {
                Icon(
                    imageVector = Icons.Default.Close,
                    contentDescription = "Dismiss Magic Bar",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(16.dp)
                )
            }
        }
    }
}

@Composable
private fun MacroEditingRow(
    onMacroAction: (String) -> Unit
) {
    val macros = listOf("Undo", "Redo", "Cut", "Copy", "Paste", "Select All", "Del Word")

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 3.dp)
            .horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        macros.forEach { action ->
            Surface(
                modifier = Modifier
                    .clip(RoundedCornerShape(6.dp))
                    .clickable { onMacroAction(action) },
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(6.dp)
            ) {
                Text(
                    text = action,
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 7.dp, vertical = 5.dp)
                )
            }
        }
    }
}

@Composable
private fun BilingualPunctuationRow(
    onPunctuationTap: (String) -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        BENGALI_PUNCTUATION.forEach { punct ->
            Surface(
                modifier = Modifier
                    .weight(1f)
                    .height(32.dp)
                    .padding(horizontal = 2.dp)
                    .clip(RoundedCornerShape(6.dp))
                    .clickable { onPunctuationTap(punct) },
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(6.dp)
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        text = punct,
                        fontSize = 15.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                }
            }
        }
    }
}

@Composable
private fun MiniHistoryDrawer(
    recordings: List<HistoryItem>,
    onItemClick: (String) -> Unit,
    onClose: () -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .height(260.dp)
            .padding(vertical = 4.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                text = "Recent Dictations (${recordings.size})",
                fontSize = 13.sp,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurface
            )

            Surface(
                modifier = Modifier
                    .clip(RoundedCornerShape(6.dp))
                    .clickable { onClose() },
                color = MaterialTheme.colorScheme.primaryContainer,
                shape = RoundedCornerShape(6.dp)
            ) {
                Text(
                    text = "✕ Back to Mic",
                    fontSize = 11.sp,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                    modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                )
            }
        }

        Spacer(modifier = Modifier.height(6.dp))

        if (recordings.isEmpty()) {
            Box(
                modifier = Modifier.fillMaxWidth().weight(1f),
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = "No dictations found yet.",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier.fillMaxWidth().weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                items(recordings) { item ->
                    Surface(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { onItemClick(item.activeText) },
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Column(modifier = Modifier.padding(8.dp)) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.SpaceBetween
                            ) {
                                Text(
                                    text = item.mode,
                                    fontSize = 10.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = MaterialTheme.colorScheme.primary
                                )
                                Text(
                                    text = "${item.durationMs / 1000}s",
                                    fontSize = 10.sp,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                            Spacer(modifier = Modifier.height(2.dp))
                            Text(
                                text = item.activeText,
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurface,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun VoiceHeroButton(
    state: VoiceKeyboardState,
    statusMessage: String?,
    audioAmplitude: Float,
    onMicTap: () -> Unit
) {
    val infiniteTransition = rememberInfiniteTransition(label = "pulseTransition")
    val pulseScale by infiniteTransition.animateFloat(
        initialValue = 1f,
        targetValue = 1.18f,
        animationSpec = infiniteRepeatable(
            animation = tween(650, easing = FastOutSlowInEasing),
            repeatMode = RepeatMode.Reverse
        ),
        label = "pulseAnim"
    )

    val buttonColor by animateColorAsState(
        targetValue = when (state) {
            VoiceKeyboardState.IDLE -> MaterialTheme.colorScheme.primary
            VoiceKeyboardState.RECORDING -> Color(0xFFEF4444)
            VoiceKeyboardState.PROCESSING -> Color(0xFF6366F1)
        },
        label = "buttonColorAnim"
    )

    val dynamicPulse = if (state == VoiceKeyboardState.RECORDING) {
        pulseScale + (audioAmplitude * 0.35f)
    } else {
        1f
    }

    Column(
        modifier = Modifier.padding(vertical = 2.dp),
        verticalArrangement = Arrangement.Top,
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Box(
            modifier = Modifier.size(86.dp),
            contentAlignment = Alignment.Center
        ) {
            if (state == VoiceKeyboardState.RECORDING) {
                // Outer amplitude ripple ring
                Box(
                    modifier = Modifier
                        .size(80.dp)
                        .scale(dynamicPulse * 1.12f)
                        .clip(CircleShape)
                        .background(Color(0xFFEF4444).copy(alpha = 0.15f + audioAmplitude * 0.2f))
                )
                // Inner pulsating ring
                Box(
                    modifier = Modifier
                        .size(72.dp)
                        .scale(dynamicPulse)
                        .clip(CircleShape)
                        .background(Color(0xFFEF4444).copy(alpha = 0.28f + audioAmplitude * 0.25f))
                )
            }

            Surface(
                modifier = Modifier
                    .size(64.dp)
                    .clip(CircleShape)
                    .clickable { onMicTap() },
                shape = CircleShape,
                color = buttonColor,
                shadowElevation = 8.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    when (state) {
                        VoiceKeyboardState.IDLE -> {
                            Icon(
                                imageVector = Icons.Default.Mic,
                                contentDescription = "Tap to speak",
                                tint = Color.White,
                                modifier = Modifier.size(32.dp)
                            )
                        }
                        VoiceKeyboardState.RECORDING -> {
                            Icon(
                                imageVector = Icons.Default.Stop,
                                contentDescription = "Stop recording",
                                tint = Color.White,
                                modifier = Modifier.size(32.dp)
                            )
                        }
                        VoiceKeyboardState.PROCESSING -> {
                            CircularProgressIndicator(
                                modifier = Modifier.size(32.dp),
                                color = Color.White,
                                strokeWidth = 3.dp
                            )
                        }
                    }
                }
            }
        }

        Spacer(modifier = Modifier.height(2.dp))

        val labelText = when {
            statusMessage != null -> statusMessage
            state == VoiceKeyboardState.IDLE -> "Tap to speak"
            state == VoiceKeyboardState.RECORDING -> "Listening..."
            state == VoiceKeyboardState.PROCESSING -> "Polishing with AI..."
            else -> "Tap to speak"
        }

        Text(
            text = labelText,
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
            color = when (state) {
                VoiceKeyboardState.RECORDING -> Color(0xFFEF4444)
                VoiceKeyboardState.PROCESSING -> MaterialTheme.colorScheme.primary
                else -> MaterialTheme.colorScheme.onSurfaceVariant
            }
        )
    }
}

@Composable
private fun KeyboardBottomBar(
    onSpace: () -> Unit,
    onBackspace: () -> Unit,
    onAction: () -> Unit,
    onCursorMoveLeft: () -> Unit,
    onCursorMoveRight: () -> Unit,
    onSwitchKeyboard: () -> Unit,
    onOpenSettings: () -> Unit,
    onSwitchToTyping: () -> Unit
) {
    var dragAccumulator by remember { mutableFloatStateOf(0f) }

    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(5.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        // Switch Keyboard (Globe)
        IconButton(
            onClick = onSwitchKeyboard,
            modifier = Modifier.size(40.dp),
            colors = IconButtonDefaults.iconButtonColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant
            )
        ) {
            Icon(
                imageVector = Icons.Default.Language,
                contentDescription = "Switch Keyboard",
                modifier = Modifier.size(19.dp)
            )
        }

        // Switch to Virtual Typing Deck Button (FlorisBoard dual-deck)
        IconButton(
            onClick = onSwitchToTyping,
            modifier = Modifier.size(40.dp),
            colors = IconButtonDefaults.iconButtonColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                contentColor = MaterialTheme.colorScheme.primary
            )
        ) {
            Icon(
                imageVector = Icons.Default.Keyboard,
                contentDescription = "Switch to Typing Keyboard",
                modifier = Modifier.size(19.dp)
            )
        }

        // Swipe Cursor Spacebar Pill
        Box(
            modifier = Modifier
                .weight(1f)
                .height(42.dp)
                .clip(RoundedCornerShape(21.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = { dragAccumulator = 0f },
                        onHorizontalDrag = { _, dragAmount ->
                            dragAccumulator += dragAmount
                            val thresholdPx = 30f
                            while (dragAccumulator > thresholdPx) {
                                onCursorMoveRight()
                                dragAccumulator -= thresholdPx
                            }
                            while (dragAccumulator < -thresholdPx) {
                                onCursorMoveLeft()
                                dragAccumulator += thresholdPx
                            }
                        }
                    )
                }
                .clickable { onSpace() },
            contentAlignment = Alignment.Center
        ) {
            Text(
                "Space  ⇄",
                fontSize = 12.sp,
                fontWeight = FontWeight.Medium,
                color = MaterialTheme.colorScheme.onSurface
            )
        }

        // Backspace
        IconButton(
            onClick = onBackspace,
            modifier = Modifier.size(40.dp),
            colors = IconButtonDefaults.iconButtonColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant
            )
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Filled.Backspace,
                contentDescription = "Backspace",
                modifier = Modifier.size(19.dp)
            )
        }

        // Action / Enter
        IconButton(
            onClick = onAction,
            modifier = Modifier.size(40.dp),
            colors = IconButtonDefaults.iconButtonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = MaterialTheme.colorScheme.onPrimary
            )
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Filled.Send,
                contentDescription = "Action / Enter",
                modifier = Modifier.size(19.dp)
            )
        }

        // Settings Shortcut
        IconButton(
            onClick = onOpenSettings,
            modifier = Modifier.size(40.dp),
            colors = IconButtonDefaults.iconButtonColors(
                containerColor = MaterialTheme.colorScheme.surfaceVariant,
                contentColor = MaterialTheme.colorScheme.onSurfaceVariant
            )
        ) {
            Icon(
                imageVector = Icons.Default.Settings,
                contentDescription = "Settings",
                modifier = Modifier.size(19.dp)
            )
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FlorisBoard-Inspired Full Virtual Typing Deck & Modular Key Composables
// ─────────────────────────────────────────────────────────────────────────────

@Composable
private fun KeyCap(
    text: String,
    modifier: Modifier = Modifier,
    onClick: () -> Unit
) {
    Surface(
        modifier = modifier
            .height(44.dp)
            .clip(RoundedCornerShape(8.dp))
            .clickable { onClick() },
        color = MaterialTheme.colorScheme.surfaceVariant,
        shape = RoundedCornerShape(8.dp),
        tonalElevation = 2.dp
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(
                text = text,
                fontSize = 17.sp,
                fontWeight = FontWeight.Medium,
                color = MaterialTheme.colorScheme.onSurface
            )
        }
    }
}

@Composable
private fun KeyRow(
    keys: List<String>,
    shiftState: ShiftState,
    onKeyClick: (String) -> Unit
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        keys.forEach { key ->
            val display = if (shiftState != ShiftState.OFF) key.uppercase() else key.lowercase()
            KeyCap(
                text = display,
                modifier = Modifier.weight(1f),
                onClick = { onKeyClick(key) }
            )
        }
    }
}

@Composable
private fun BengaliKeyRow(
    keys: List<String>,
    onKeyClick: (String) -> Unit
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        keys.forEach { key ->
            KeyCap(
                text = key,
                modifier = Modifier.weight(1f),
                onClick = { onKeyClick(key) }
            )
        }
    }
}

@Composable
fun TypingDeckView(
    onCharTap: (String) -> Unit,
    onSpace: () -> Unit,
    onBackspace: () -> Unit,
    onAction: () -> Unit,
    onCursorMoveLeft: () -> Unit,
    onCursorMoveRight: () -> Unit,
    onSwitchKeyboard: () -> Unit,
    onVoiceTap: () -> Unit
) {
    var layoutMode by remember { mutableStateOf(TypingLayoutMode.QWERTY) }
    var shiftState by remember { mutableStateOf(ShiftState.OFF) }
    val view = LocalView.current

    val hapticTap = {
        try {
            view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
        } catch (_: Throwable) {}
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp)
    ) {
        when (layoutMode) {
            TypingLayoutMode.QWERTY -> {
                val row1 = listOf("q", "w", "e", "r", "t", "y", "u", "i", "o", "p")
                KeyRow(keys = row1, shiftState = shiftState, onKeyClick = {
                    hapticTap()
                    val char = if (shiftState != ShiftState.OFF) it.uppercase() else it.lowercase()
                    onCharTap(char)
                    if (shiftState == ShiftState.ONCE) shiftState = ShiftState.OFF
                })

                val row2 = listOf("a", "s", "d", "f", "g", "h", "j", "k", "l")
                KeyRow(keys = row2, shiftState = shiftState, onKeyClick = {
                    hapticTap()
                    val char = if (shiftState != ShiftState.OFF) it.uppercase() else it.lowercase()
                    onCharTap(char)
                    if (shiftState == ShiftState.ONCE) shiftState = ShiftState.OFF
                })

                val row3 = listOf("z", "x", "c", "v", "b", "n", "m")
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    // Shift Key
                    Surface(
                        modifier = Modifier
                            .weight(1.4f)
                            .height(44.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .clickable {
                                hapticTap()
                                shiftState = when (shiftState) {
                                    ShiftState.OFF -> ShiftState.ONCE
                                    ShiftState.ONCE -> ShiftState.CAPS_LOCK
                                    ShiftState.CAPS_LOCK -> ShiftState.OFF
                                }
                            },
                        color = if (shiftState != ShiftState.OFF) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(8.dp),
                        tonalElevation = 2.dp
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(
                                text = when (shiftState) {
                                    ShiftState.OFF -> "⇧"
                                    ShiftState.ONCE -> "⬆"
                                    ShiftState.CAPS_LOCK -> "⇪"
                                },
                                fontSize = 16.sp,
                                fontWeight = FontWeight.Bold,
                                color = if (shiftState != ShiftState.OFF) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }

                    row3.forEach { key ->
                        KeyCap(
                            text = if (shiftState != ShiftState.OFF) key.uppercase() else key.lowercase(),
                            modifier = Modifier.weight(1f),
                            onClick = {
                                hapticTap()
                                val char = if (shiftState != ShiftState.OFF) key.uppercase() else key.lowercase()
                                onCharTap(char)
                                if (shiftState == ShiftState.ONCE) shiftState = ShiftState.OFF
                            }
                        )
                    }

                    // Backspace Key
                    Surface(
                        modifier = Modifier
                            .weight(1.4f)
                            .height(44.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .clickable {
                                hapticTap()
                                onBackspace()
                            },
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(8.dp),
                        tonalElevation = 2.dp
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.Backspace,
                                contentDescription = "Backspace",
                                modifier = Modifier.size(20.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                }
            }

            TypingLayoutMode.BENGALI -> {
                val bnRow1 = listOf("ক", "খ", "গ", "ঘ", "ঙ", "চ", "ছ", "জ", "ঝ", "ঞ")
                val bnRow2 = listOf("ট", "ঠ", "ড", "ঢ", "ণ", "ত", "থ", "দ", "ধ", "ন")
                val bnRow3 = listOf("প", "ফ", "ব", "ভ", "ম", "য", "র", "ল", "শ", "স")
                val bnRow4 = listOf("া", "ি", "ী", "ু", "ূ", "ে", "ৈ", "ো", "ৌ", "্", "ৎ", "ং", "ঃ", "ঁ")

                BengaliKeyRow(keys = bnRow1, onKeyClick = { hapticTap(); onCharTap(it) })
                BengaliKeyRow(keys = bnRow2, onKeyClick = { hapticTap(); onCharTap(it) })

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    bnRow3.forEach { key ->
                        KeyCap(
                            text = key,
                            modifier = Modifier.weight(1f),
                            onClick = { hapticTap(); onCharTap(key) }
                        )
                    }
                    Surface(
                        modifier = Modifier
                            .weight(1.3f)
                            .height(44.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { hapticTap(); onBackspace() },
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(8.dp),
                        tonalElevation = 2.dp
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.Backspace,
                                contentDescription = "Backspace",
                                modifier = Modifier.size(20.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    bnRow4.forEach { sign ->
                        Surface(
                            modifier = Modifier
                                .height(38.dp)
                                .clip(RoundedCornerShape(6.dp))
                                .clickable { hapticTap(); onCharTap(sign) },
                            color = MaterialTheme.colorScheme.surfaceVariant,
                            shape = RoundedCornerShape(6.dp),
                            tonalElevation = 2.dp
                        ) {
                            Box(modifier = Modifier.padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                                Text(
                                    text = sign,
                                    fontSize = 15.sp,
                                    fontWeight = FontWeight.Bold,
                                    color = MaterialTheme.colorScheme.onSurface
                                )
                            }
                        }
                    }
                }
            }

            TypingLayoutMode.NUM_SYMBOLS -> {
                val numRow1 = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "0")
                val numRow2 = listOf("@", "#", "$", "%", "&", "-", "+", "(", ")", "/")
                val numRow3 = listOf("=", "*", "\"", "'", ":", ";", "!", "?")

                KeyRow(keys = numRow1, shiftState = ShiftState.OFF, onKeyClick = { hapticTap(); onCharTap(it) })
                KeyRow(keys = numRow2, shiftState = ShiftState.OFF, onKeyClick = { hapticTap(); onCharTap(it) })

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    numRow3.forEach { key ->
                        KeyCap(
                            text = key,
                            modifier = Modifier.weight(1f),
                            onClick = { hapticTap(); onCharTap(key) }
                        )
                    }
                    Surface(
                        modifier = Modifier
                            .weight(1.4f)
                            .height(44.dp)
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { hapticTap(); onBackspace() },
                        color = MaterialTheme.colorScheme.surfaceVariant,
                        shape = RoundedCornerShape(8.dp),
                        tonalElevation = 2.dp
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                imageVector = Icons.AutoMirrored.Filled.Backspace,
                                contentDescription = "Backspace",
                                modifier = Modifier.size(20.dp),
                                tint = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                }
            }
        }

        // Bottom Bar of Typing Deck
        var dragAccumulator by remember { mutableFloatStateOf(0f) }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(5.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            // ?123 / ABC Toggle
            Surface(
                modifier = Modifier
                    .weight(1.3f)
                    .height(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .clickable {
                        hapticTap()
                        layoutMode = if (layoutMode == TypingLayoutMode.NUM_SYMBOLS) {
                            TypingLayoutMode.QWERTY
                        } else {
                            TypingLayoutMode.NUM_SYMBOLS
                        }
                    },
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(8.dp),
                tonalElevation = 2.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        text = if (layoutMode == TypingLayoutMode.NUM_SYMBOLS) "ABC" else "?123",
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            // বাংলা / EN Toggle
            Surface(
                modifier = Modifier
                    .weight(1.3f)
                    .height(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .clickable {
                        hapticTap()
                        layoutMode = if (layoutMode == TypingLayoutMode.BENGALI) {
                            TypingLayoutMode.QWERTY
                        } else {
                            TypingLayoutMode.BENGALI
                        }
                    },
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(8.dp),
                tonalElevation = 2.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        text = if (layoutMode == TypingLayoutMode.BENGALI) "EN" else "বাং",
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
            }

            // Quick Voice Trigger
            Surface(
                modifier = Modifier
                    .size(44.dp)
                    .clip(CircleShape)
                    .clickable {
                        hapticTap()
                        onVoiceTap()
                    },
                color = MaterialTheme.colorScheme.primaryContainer,
                shape = CircleShape,
                tonalElevation = 2.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(
                        imageVector = Icons.Default.Mic,
                        contentDescription = "Quick Voice",
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onPrimaryContainer
                    )
                }
            }

            // Spacebar with Swipe Cursor Control
            Box(
                modifier = Modifier
                    .weight(3.5f)
                    .height(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .pointerInput(Unit) {
                        detectHorizontalDragGestures(
                            onDragStart = { dragAccumulator = 0f },
                            onHorizontalDrag = { _, dragAmount ->
                                dragAccumulator += dragAmount
                                val thresholdPx = 28f
                                while (dragAccumulator > thresholdPx) {
                                    onCursorMoveRight()
                                    dragAccumulator -= thresholdPx
                                }
                                while (dragAccumulator < -thresholdPx) {
                                    onCursorMoveLeft()
                                    dragAccumulator += thresholdPx
                                }
                            }
                        )
                    }
                    .clickable {
                        hapticTap()
                        onSpace()
                    },
                contentAlignment = Alignment.Center
            ) {
                Text(
                    text = when (layoutMode) {
                        TypingLayoutMode.BENGALI -> "স্পেস  ⇄"
                        TypingLayoutMode.NUM_SYMBOLS -> "Space  ⇄"
                        else -> "Space  ⇄"
                    },
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Medium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }

            // Period / Bengali Dari
            Surface(
                modifier = Modifier
                    .weight(1f)
                    .height(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .clickable {
                        hapticTap()
                        val punct = if (layoutMode == TypingLayoutMode.BENGALI) "।" else "."
                        onCharTap(punct)
                    },
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(8.dp),
                tonalElevation = 2.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        text = if (layoutMode == TypingLayoutMode.BENGALI) "।" else ".",
                        fontSize = 16.sp,
                        fontWeight = FontWeight.Bold,
                        color = MaterialTheme.colorScheme.onSurface
                    )
                }
            }

            // Enter / Action Key
            Surface(
                modifier = Modifier
                    .weight(1.4f)
                    .height(44.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .clickable {
                        hapticTap()
                        onAction()
                    },
                color = MaterialTheme.colorScheme.primary,
                shape = RoundedCornerShape(8.dp),
                tonalElevation = 4.dp
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.Send,
                        contentDescription = "Send",
                        modifier = Modifier.size(20.dp),
                        tint = MaterialTheme.colorScheme.onPrimary
                    )
                }
            }
        }
    }
}
