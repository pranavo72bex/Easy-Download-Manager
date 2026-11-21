// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android;

import android.content.Intent;
import android.content.res.Configuration;
import android.graphics.Color;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

import dev.gpui.mobile.GpuiInputActivity;

/**
 * dm's activity: gpui-mobile's, with a new intent handed to Rust (gaps row N5) and the system
 * bars told to Rust (the phone's layout). A link shared while dm is open comes to the running
 * activity (singleTask) as {@code onNewIntent}, which NativeActivity keeps from native code;
 * here it becomes the activity's intent and Rust reads it as it reads the first one
 * ({@code src/android/intent.rs}).
 *
 * <p>dm's window is drawn over the whole screen on every Android version, under the status bar
 * and the navigation bar, and what they cover is handed to Rust as GPUI's safe area, which
 * dm's views keep their controls out of. NativeActivity's own content rect is the whole screen
 * then, so it tells nothing. The bars' icons are dark on a light screen and light on a dark
 * one, following the phone's dark theme as dm's colours do.
 */
public class DmActivity extends GpuiInputActivity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        Window window = getWindow();
        View decor = window.getDecorView();
        drawnEdgeToEdge(window, decor);
        decor.setOnApplyWindowInsetsListener((view, insets) -> {
            told(insets);
            return view.onApplyWindowInsets(insets);
        });
        barIconsFor(getResources().getConfiguration());
        decor.requestApplyInsets();
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        barIconsFor(configuration);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        newIntent();
    }

    /** The window under the bars, the bars see-through. */
    @SuppressWarnings("deprecation")
    private static void drawnEdgeToEdge(Window window, View decor) {
        window.setStatusBarColor(Color.TRANSPARENT);
        window.setNavigationBarColor(Color.TRANSPARENT);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
        } else {
            decor.setSystemUiVisibility(decor.getSystemUiVisibility()
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
    }

    /** What the bars and a cut-out cover, handed to Rust. */
    @SuppressWarnings("deprecation")
    private static void told(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            android.graphics.Insets bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            systemInsets(bars.top, bars.bottom, bars.left, bars.right);
        } else {
            systemInsets(insets.getSystemWindowInsetTop(), insets.getSystemWindowInsetBottom(),
                    insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetRight());
        }
    }

    /** Dark icons on the bars where the phone's theme is light, light ones where it is dark. */
    @SuppressWarnings("deprecation")
    private void barIconsFor(Configuration configuration) {
        boolean dark = (configuration.uiMode & Configuration.UI_MODE_NIGHT_MASK)
                == Configuration.UI_MODE_NIGHT_YES;
        Window window = getWindow();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController bars = window.getInsetsController();
            if (bars == null) return;
            int light = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                    | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
            bars.setSystemBarsAppearance(dark ? 0 : light, light);
        } else {
            View decor = window.getDecorView();
            int light = View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
                    | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            int flags = decor.getSystemUiVisibility();
            decor.setSystemUiVisibility(dark ? flags & ~light : flags | light);
        }
    }

    /** {@code Java_com_dm_android_DmActivity_newIntent} in {@code src/android/natives.rs}. */
    private static native void newIntent();

    /** {@code Java_com_dm_android_DmActivity_systemInsets} in {@code src/android/natives.rs}. */
    private static native void systemInsets(int top, int bottom, int left, int right);
}
