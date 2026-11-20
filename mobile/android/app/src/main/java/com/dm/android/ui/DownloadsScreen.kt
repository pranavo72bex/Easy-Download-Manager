// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dm.android.R
import com.dm.android.data.Format
import com.dm.android.data.ListFilter
import com.dm.android.data.StateFilter
import uniffi.dm_android.RowState
import uniffi.dm_android.Row as Download

/** The window: Options while open, else the list with its sheets; and the snackbar's words. */
@Composable
fun DmScreens(model: DownloadsViewModel) {
    val options by model.options.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val context = LocalContext.current
    LaunchedEffect(model) {
        model.messages.collect { snackbar.showSnackbar(it.words(context)) }
    }
    val open = options
    if (open != null) OptionsScreen(open, model, snackbar) else DownloadsScreen(model, snackbar)
}

/** The list (gaps row N7): its chips, rows, the top bar's commands and Add. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DownloadsScreen(model: DownloadsViewModel, snackbar: SnackbarHostState) {
    val rows by model.rows.collectAsStateWithLifecycle()
    val filter by model.filter.collectAsStateWithLifecycle()
    val failure by model.failure.collectAsStateWithLifecycle()
    val selected by model.selected.collectAsStateWithLifecycle()
    val draft by model.addDraft.collectAsStateWithLifecycle()
    val offer by model.clipboardOffer.collectAsStateWithLifecycle()
    Scaffold(
        topBar = { TopAppBar(title = { Text(stringResource(R.string.app_name)) }, actions = { ListActions(model) }) },
        floatingActionButton = {
            FloatingActionButton(onClick = { model.openAdd() }) {
                Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.action_add))
            }
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            offer?.let { text -> ClipboardOffer(text, { model.openAdd(text) }, model::dismissClipboard) }
            FilterChips(ListFilter.categories(rows), filter, model::setFilter)
            failure?.let {
                Text(
                    stringResource(R.string.engine_failed, it),
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(16.dp),
                )
            }
            DownloadList(rows.filter { filter.shows(it) }) { model.select(it.id) }
        }
    }
    selected?.let { DetailsSheet(it, model) }
    draft?.let { AddSheet(it, model) }
}

@Composable
private fun ListActions(model: DownloadsViewModel) {
    var menu by remember { mutableStateOf(false) }
    IconButton(onClick = model::openOptions) {
        Icon(Icons.Filled.Settings, contentDescription = stringResource(R.string.action_options))
    }
    Box {
        IconButton(onClick = { menu = true }) {
            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.action_more))
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            MenuLine(R.string.action_stop_all) { menu = false; model.pauseAll() }
            MenuLine(R.string.action_resume_all) { menu = false; model.resumeAll() }
            MenuLine(R.string.action_clear_finished) { menu = false; model.clearFinished() }
            MenuLine(R.string.action_options) { menu = false; model.openOptions() }
        }
    }
}

@Composable
private fun MenuLine(words: Int, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(stringResource(words)) }, onClick = onClick)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FilterChips(categories: List<String>, filter: ListFilter, onChange: (ListFilter) -> Unit) {
    val states = listOf(
        StateFilter.ALL to R.string.filter_all,
        StateFilter.RUNNING to R.string.filter_running,
        StateFilter.FINISHED to R.string.filter_finished,
        StateFilter.FAILED to R.string.filter_failed,
    )
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        states.forEach { (state, words) ->
            FilterChip(
                selected = filter.state == state,
                onClick = { onChange(filter.copy(state = state)) },
                label = { Text(stringResource(words)) },
                modifier = Modifier.testTag("chip-${state.name}"),
            )
        }
        categories.forEach { category ->
            val on = filter.category == category
            FilterChip(
                selected = on,
                onClick = { onChange(filter.copy(category = if (on) null else category)) },
                label = { Text(Format.label(category)) },
            )
        }
    }
}

@Composable
private fun DownloadList(rows: List<Download>, onOpen: (Download) -> Unit) {
    if (rows.isEmpty()) {
        Text(
            stringResource(R.string.list_empty),
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.padding(24.dp),
        )
        return
    }
    LazyColumn(Modifier.fillMaxSize().testTag("downloads")) {
        items(rows, key = { it.id.toLong() }) { row ->
            DownloadLine(row) { onOpen(row) }
            HorizontalDivider()
        }
    }
}

/** One row: name, bar, size of total, and speed and time left or the state. */
@Composable
fun DownloadLine(row: Download, onClick: () -> Unit) {
    Column(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 10.dp)) {
        Text(row.name, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.height(6.dp))
        ProgressBar(row)
        Spacer(Modifier.height(4.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            Text(
                Format.progress(row.downloaded.toLong(), row.total?.toLong(), row.estimated),
                style = MaterialTheme.typography.bodySmall,
            )
            Text(statusLine(row), style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun ProgressBar(row: Download) {
    val fraction = Format.fraction(row.downloaded.toLong(), row.total?.toLong())
    val modifier = Modifier.fillMaxWidth()
    when {
        row.state == RowState.COMPLETED -> LinearProgressIndicator(progress = { 1f }, modifier = modifier)
        fraction == null && row.state == RowState.RUNNING -> LinearProgressIndicator(modifier = modifier)
        else -> LinearProgressIndicator(progress = { fraction ?: 0f }, modifier = modifier)
    }
}

/** "1.2 MB/s · 3m 05s" while running, else the state's word. */
@Composable
private fun statusLine(row: Download): String {
    if (row.state != RowState.RUNNING) return stateWord(row.state)
    val speed = Format.speed(row.speed)
    return row.etaSeconds?.let { "$speed · ${Format.eta(it.toLong())}" } ?: speed
}

@Composable
private fun ClipboardOffer(text: String, onAdd: () -> Unit, onDismiss: () -> Unit) {
    Surface(tonalElevation = 2.dp, modifier = Modifier.fillMaxWidth().padding(12.dp)) {
        Column(Modifier.padding(12.dp)) {
            Text(stringResource(R.string.clipboard_offer), style = MaterialTheme.typography.titleSmall)
            Text(text, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall)
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onDismiss) { Text(stringResource(R.string.dismiss)) }
                TextButton(onClick = onAdd) { Text(stringResource(R.string.action_add)) }
            }
        }
    }
}
