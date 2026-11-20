// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.dm.android.R
import com.dm.android.data.DownloadsSource
import com.dm.android.data.Links
import com.dm.android.data.ListFilter
import com.dm.android.data.NotANumber
import com.dm.android.data.Outcome
import com.dm.android.data.SettingsForm
import com.dm.android.data.SettingsSection
import com.dm.android.data.active
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.dm_android.Row

/** Something said once in the snackbar: dm's own words, or the app's from strings.xml. */
sealed interface UiMessage {
    data class Text(val text: String) : UiMessage
    data class Res(@StringRes val id: Int, val arg: String? = null) : UiMessage
    data class Plural(@PluralsRes val id: Int, val count: Int) : UiMessage
}

/** Options while open: the JSON it was read from, its lines as edited, and a refusal. */
data class OptionsState(
    val original: String,
    val sections: List<SettingsSection>,
    val saving: Boolean = false,
    val error: UiMessage? = null,
)

/**
 * The screens' state over a [DownloadsSource]: the list, read again on each change the engine
 * tells (debounced) and every second while something runs; the Add sheet; Options.
 */
class DownloadsViewModel(
    private val source: DownloadsSource,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val _rows = MutableStateFlow<List<Row>>(emptyList())
    val rows: StateFlow<List<Row>> = _rows.asStateFlow()
    private val _filter = MutableStateFlow(ListFilter())
    val filter: StateFlow<ListFilter> = _filter.asStateFlow()
    private val _failure = MutableStateFlow<String?>(null)
    val failure: StateFlow<String?> = _failure.asStateFlow()
    private val _selected = MutableStateFlow<ULong?>(null)
    val selected: StateFlow<Row?> = combine(_rows, _selected) { rows, id ->
        id?.let { wanted -> rows.firstOrNull { it.id == wanted } }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    private val _addDraft = MutableStateFlow<String?>(null)
    /** The Add sheet's text; null while it is closed. */
    val addDraft: StateFlow<String?> = _addDraft.asStateFlow()
    private val _downloadDir = MutableStateFlow<String?>(null)
    val downloadDir: StateFlow<String?> = _downloadDir.asStateFlow()
    private val _clipboardOffer = MutableStateFlow<String?>(null)
    val clipboardOffer: StateFlow<String?> = _clipboardOffer.asStateFlow()
    private val _options = MutableStateFlow<OptionsState?>(null)
    val options: StateFlow<OptionsState?> = _options.asStateFlow()
    private val _messages = Channel<UiMessage>(Channel.BUFFERED)
    val messages: Flow<UiMessage> = _messages.receiveAsFlow()
    private val offered = mutableSetOf<String>()

    init {
        viewModelScope.launch {
            source.ticks.collectLatest {
                delay(DEBOUNCE_MS)
                refresh()
            }
        }
        viewModelScope.launch {
            while (isActive) {
                delay(POLL_MS)
                if (_rows.value.any { it.active() }) refresh()
            }
        }
        viewModelScope.launch {
            withContext(io) { source.takeNotices() }.forEach { say(UiMessage.Text(it)) }
        }
    }

    suspend fun refresh() {
        val (rows, failure) = withContext(io) { source.list() to source.startFailure() }
        _rows.value = rows
        _failure.value = failure
    }

    fun setFilter(filter: ListFilter) {
        _filter.value = filter
    }

    fun select(id: ULong?) {
        _selected.value = id
    }

    fun pause(id: ULong) = act { it.pause(id) }
    fun resume(id: ULong) = act { it.resume(id) }
    fun retry(id: ULong) = act { it.retry(id) }
    fun cancel(id: ULong) = act { it.cancel(id) }

    fun remove(id: ULong) {
        _selected.value = null
        act { it.remove(id) }
    }

    fun pauseAll() = counted(R.plurals.stopped) { it.pauseAll() }
    fun resumeAll() = counted(R.plurals.resumed) { it.resumeAll() }
    fun clearFinished() = counted(R.plurals.cleared) { it.clearFinished() }

    fun open(path: String) = said { it.open(path) }
    fun reveal(path: String) = said { it.reveal(path) }

    /** Opens the Add sheet with `text` (a share's, the clipboard's, or nothing). */
    fun openAdd(text: String = "") {
        _clipboardOffer.value = null
        _addDraft.value = text
        viewModelScope.launch {
            val json = withContext(io) { source.settingsJson() }
            if (json is Outcome.Done) _downloadDir.value = Links.text(json.value, "download_dir")
        }
    }

    fun editAdd(text: String) {
        if (_addDraft.value != null) _addDraft.value = text
    }

    fun closeAdd() {
        _addDraft.value = null
    }

    fun submitAdd() {
        val text = _addDraft.value ?: return
        viewModelScope.launch {
            when (val outcome = withContext(io) { source.add(text) }) {
                is Outcome.Done -> {
                    _addDraft.value = null
                    say(UiMessage.Plural(R.plurals.added, outcome.value.added))
                    if (outcome.value.skipped.isNotEmpty()) {
                        say(UiMessage.Res(R.string.left_out, outcome.value.skipped.joinToString("; ")))
                    }
                    refresh()
                }
                is Outcome.Refused -> say(UiMessage.Text(outcome.reason))
            }
        }
    }

    /** A problem with what was handed to dm, said before any Add sheet. */
    fun tell(message: UiMessage) = say(message)

    /**
     * Offers a link on the clipboard once, where Options' Watch clipboard is on. Called when
     * the window gains focus: Android gives the clipboard to the app in front only.
     */
    fun checkClipboard() {
        if (_addDraft.value != null || _options.value != null) return
        viewModelScope.launch {
            val watching = withContext(io) {
                (source.settingsJson() as? Outcome.Done)?.let { Links.switchOn(it.value, "watch_clipboard") }
            }
            if (watching != true) return@launch
            val text = source.clipboard()?.trim().orEmpty()
            if (Links.found(text).isEmpty() || !offered.add(text)) return@launch
            _clipboardOffer.value = text
        }
    }

    fun dismissClipboard() {
        _clipboardOffer.value = null
    }

    fun openOptions() {
        viewModelScope.launch {
            when (val json = withContext(io) { source.settingsJson() }) {
                is Outcome.Done -> _options.value = optionsOf(json.value)
                is Outcome.Refused -> say(UiMessage.Text(json.reason))
            }
        }
    }

    fun editOption(key: String, value: String) {
        val now = _options.value ?: return
        _options.value = now.copy(
            error = null,
            sections = now.sections.map { section ->
                section.copy(fields = section.fields.map { if (it.key == key) it.copy(value = value) else it })
            },
        )
    }

    fun closeOptions() {
        _options.value = null
    }

    fun saveOptions() {
        val now = _options.value ?: return
        val json = try {
            SettingsForm.write(now.original, now.sections.flatMap { it.fields })
        } catch (e: NotANumber) {
            _options.value = now.copy(error = UiMessage.Res(R.string.options_needs_number, e.label))
            return
        }
        _options.value = now.copy(saving = true, error = null)
        viewModelScope.launch {
            when (val outcome = withContext(io) { source.saveSettings(json) }) {
                is Outcome.Done -> {
                    _options.value = null
                    say(UiMessage.Res(R.string.options_saved))
                }
                is Outcome.Refused ->
                    _options.value = now.copy(saving = false, error = UiMessage.Text(outcome.reason))
            }
        }
    }

    private fun optionsOf(json: String): OptionsState? = try {
        OptionsState(json, SettingsForm.read(json))
    } catch (e: IllegalArgumentException) {
        say(UiMessage.Text(e.message.orEmpty()))
        null
    }

    private fun act(call: (DownloadsSource) -> Unit) {
        viewModelScope.launch {
            withContext(io) { call(source) }
            refresh()
        }
    }

    private fun counted(@PluralsRes id: Int, call: (DownloadsSource) -> Int) {
        viewModelScope.launch {
            val n = withContext(io) { call(source) }
            say(UiMessage.Plural(id, n))
            refresh()
        }
    }

    private fun said(call: (DownloadsSource) -> String?) {
        viewModelScope.launch {
            withContext(io) { call(source) }?.let { say(UiMessage.Text(it)) }
        }
    }

    private fun say(message: UiMessage) {
        _messages.trySend(message)
    }

    /** Makes the view model over the app's source. */
    class Factory(private val source: DownloadsSource) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = DownloadsViewModel(source) as T
    }

    private companion object {
        const val DEBOUNCE_MS = 150L
        const val POLL_MS = 1000L
    }
}
