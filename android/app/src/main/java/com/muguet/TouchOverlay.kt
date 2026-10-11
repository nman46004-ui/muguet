package com.muguet

import android.content.Context
import android.content.SharedPreferences
import android.graphics.*
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.core.view.ViewCompat

/**
 * TouchOverlay - Android equivalent of iOS touch overlay
 * Renders virtual joystick, action buttons, nav D-pad, handles all touch input
 */
class TouchOverlay @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : ViewGroup(context, attrs, defStyleAttr) {

    companion object {
        // Native methods
        external fun nativeInit(context: Context, window: android.view.Window, scale: Float)
        external fun nativeTouch(touchId: Long, x: Float, y: Float, action: Int)
        external fun nativeUpdate(hide: Boolean, inCar: Boolean)
        external fun nativeResize(width: Int, height: Int, scale: Float)
        external fun nativeRelease()
        
        // Input bridges from native to Kotlin
        external fun inputMouseDelta(dx: Float, dy: Float)
        external fun inputKey(key: String, pressed: Boolean)
        external fun inputButton(button: Int, pressed: Boolean)
        external fun mpKeyPressed(key: String)
    }

    // Read touch sensitivity from shared preferences (set by launcher)
    private var touchSens: Float by lazy {
        val prefs = context.getSharedPreferences("muguet_config", Context.MODE_PRIVATE)
        prefs.getFloat("touch_sens", 2.0f)
    }
    
    fun refreshTouchSens() {
        val prefs = context.getSharedPreferences("muguet_config", Context.MODE_PRIVATE)
        touchSens = prefs.getFloat("touch_sens", 2.0f)
    }

    // Button specs (mirroring iOS)
    private data class ButtonSpec(
        val anchor: Anchor,
        val radius: Float,
        val onFootLabel: String?,
        val onFootAction: Action?,
        val inCarLabel: String?,
        val inCarAction: Action?,
        val isNav: Boolean = false
    )

    private enum class Anchor {
        BottomRight, TopLeft, TopRight, Nav
    }

    private enum class Action {
        Key(String), Mouse(Int), AimToggle, KeyNav(String), NavToggle, Horn
    }

    // Button layout
    private val buttonSpecs = listOf(
        ButtonSpec(Anchor.BottomRight, 40f, "FIRE", Action.Mouse(1), "GAS", Action.Key("KeyW")),
        ButtonSpec(Anchor.BottomRight, 31f, "AIM", Action.AimToggle, "BRAKE", Action.Key("KeyS")),
        ButtonSpec(Anchor.BottomRight, 28f, "JUMP", Action.Key("Space"), "H.BRAKE", Action.Key("Space")),
        ButtonSpec(Anchor.BottomRight, 26f, "ENTER", Action.Key("KeyF"), "EXIT", Action.Key("KeyF")),
        ButtonSpec(Anchor.BottomRight, 24f, "RELOAD", Action.Key("KeyR"), "HORN", Action.Horn),
        ButtonSpec(Anchor.TopRight, 22f, "WEAPON", Action.Key("Tab"), "WEAPON", Action.Key("Tab")),
        ButtonSpec(Anchor.TopRight, 22f, "COVER", Action.Key("KeyQ"), "RADIO", Action.Key("KeyQ")),
        ButtonSpec(Anchor.TopRight, 22f, "CROUCH", Action.Key("ControlLeft"), "LOOK\nBACK", Action.Key("KeyC")),
        ButtonSpec(Anchor.TopRight, 22f, "CAM", Action.Key("KeyV"), "CAM", Action.Key("KeyV")),
        ButtonSpec(Anchor.TopLeft, 22f, "PAUSE", Action.KeyNav("Escape"), "PAUSE", Action.KeyNav("Escape")),
        ButtonSpec(Anchor.TopLeft, 22f, "PHONE", Action.KeyNav("ArrowUp"), "PHONE", Action.KeyNav("ArrowUp")),
        ButtonSpec(Anchor.TopLeft, 22f, "MENU\nKEYS", Action.NavToggle, "MENU\nKEYS", Action.NavToggle),
        ButtonSpec(Anchor.TopLeft, 22f, "SWITCH", Action.Key("AltLeft"), "SWITCH", Action.Key("AltLeft")),
        // Nav buttons
        ButtonSpec(Anchor.Nav, 21f, "◀", Action.Key("ArrowLeft"), "◀", Action.Key("ArrowLeft"), true),
        ButtonSpec(Anchor.Nav, 21f, "▲", Action.Key("ArrowUp"), "▲", Action.Key("ArrowUp"), true),
        ButtonSpec(Anchor.Nav, 21f, "▼", Action.Key("ArrowDown"), "▼", Action.Key("ArrowDown"), true),
        ButtonSpec(Anchor.Nav, 21f, "▶", Action.Key("ArrowRight"), "▶", Action.Key("ArrowRight"), true),
        ButtonSpec(Anchor.Nav, 21f, "OK", Action.Key("Enter"), "OK", Action.Key("Enter"), true),
        ButtonSpec(Anchor.Nav, 21f, "BACK", Action.Key("Backspace"), "BACK", Action.Key("Backspace"), true),
    )

    private const val STICK_RADIUS = 58f
    private const val KNOB_RADIUS = 26f

    // View components
    private val buttonViews = mutableMapOf<Int, Pair<View, TextView>>()
    private var stickBase: View? = null
    private var stickKnob: View? = null
    private var stickHome = Pair(0f, 0f)

    // State
    private var scale = 1f
    private var safeArea = android.graphics.Rect(0, 0, 0, 0)
    private val grips = mutableMapOf<Int, Grip>()
    private val heldKeys = mutableSetOf<String>()
    private val keySince = mutableMapOf<String, Long>()
    private val mouseSince = LongArray(2) { System.currentTimeMillis() }
    private var lingering = false
    private var mouseButtons = 0
    private var aimLatched = false
    private var navMode = false
    private var inCar = false
    private var hidden = false

    private enum class GripType { Stick, Look, Button }
    private data class Grip(
        val type: GripType,
        val index: Int = -1,
        var origin: Pair<Float, Float> = Pair(0f, 0f),
        var current: Pair<Float, Float> = Pair(0f, 0f),
        var start: Pair<Float, Float> = Pair(0f, 0f),
        var last: Pair<Float, Float> = Pair(0f, 0f)
    )

    init {
        setWillNotDraw(false)
        ViewCompat.setImportantForAccessibility(this, ViewCompat.IMPORTANT_FOR_ACCESSIBILITY_NO)
    }

    fun initialize(window: android.view.Window) {
        val density = resources.displayMetrics.density
        scale = density
        nativeInit(context, window, scale)
        setupViews()
    }

    private fun setupViews() {
        // Create button views
        for ((i, spec) in buttonSpecs.withIndex()) {
            val btn = View(context).apply {
                isClickable = false
                background = createCircleDrawable(0x2EFFFFFF.toInt()) // 0.18 alpha white
                setLayerType(LAYER_TYPE_HARDWARE, null)
            }
            
            val label = TextView(context).apply {
                text = spec.onFootLabel ?: ""
                textSize = (spec.radius * 0.42f).coerceIn(9f, 15f)
                setTextColor(0xE6FFFFFF.toInt()) // 0.9 alpha white
                gravity = Gravity.CENTER
                maxLines = 2
                setTypeface(Typeface.DEFAULT_BOLD)
            }
            
            addView(btn)
            addView(label)
            buttonViews[i] = btn to label
        }

        // Stick base
        stickBase = View(context).apply {
            background = createCircleDrawable(0x1FFFFF.toInt()) // 0.12 alpha
            setLayerType(LAYER_TYPE_HARDWARE, null)
        }
        addView(stickBase!!)

        // Stick knob
        stickKnob = View(context).apply {
            background = createCircleDrawable(0x4CFFFFFF.toInt()) // 0.3 alpha
            setLayerType(LAYER_TYPE_HARDWARE, null)
        }
        addView(stickKnob!!)

        relabel()
    }

    private fun createCircleDrawable(color: Int): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(color)
        }
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val width = r - l
        val height = b - t
        layoutViews(width, height)
    }

    private fun layoutViews(width: Int, height: Int) {
        val sx = safeArea.left.toFloat()
        val sy = safeArea.top.toFloat()
        val sw = safeArea.width().toFloat()
        val sh = safeArea.height().toFloat()

        val place = { spec: ButtonSpec, dx: Float, dy: Float ->
            when (spec.anchor) {
                Anchor.BottomRight -> Pair(sx + sw - dx, sy + sh - dy)
                Anchor.TopLeft -> Pair(sx + dx, sy + dy)
                Anchor.TopRight -> Pair(sx + sw - dx, sy + dy)
                Anchor.Nav -> Pair(sx + sw / 2f + dx, sy + 30f + dy)
            }
        }

        // Position buttons with hardcoded positions matching iOS
        val positions = mapOf(
            0 to Pair(62f, 68f),   // FIRE
            1 to Pair(158f, 46f),  // AIM
            2 to Pair(58f, 168f),  // JUMP
            3 to Pair(140f, 132f), // ENTER
            4 to Pair(222f, 98f),  // RELOAD
            5 to Pair(30f, 30f),   // WEAPON
            6 to Pair(84f, 30f),   // COVER
            7 to Pair(138f, 30f),  // CROUCH
            8 to Pair(192f, 30f),  // CAM
            9 to Pair(30f, 30f),   // PAUSE
            10 to Pair(84f, 30f),  // PHONE
            11 to Pair(138f, 30f), // MENU KEYS
            12 to Pair(192f, 30f), // SWITCH
            13 to Pair(-135f, 0f), // ◀
            14 to Pair(-87f, 0f),  // ▲
            15 to Pair(-39f, 0f),  // ▼
            16 to Pair(9f, 0f),    // ▶
            17 to Pair(63f, 0f),   // OK
            18 to Pair(117f, 0f),  // BACK
        )

        for ((i, spec) in buttonSpecs.withIndex()) {
            val (btn, label) = buttonViews[i]!!
            val (dx, dy) = positions[i] ?: Pair(0f, 0f)
            val center = when (spec.anchor) {
                Anchor.BottomRight -> Pair(sx + sw - dx, sy + sh - dy)
                Anchor.TopLeft -> Pair(sx + dx, sy + dy)
                Anchor.TopRight -> Pair(sx + sw - dx, sy + dy)
                Anchor.Nav -> Pair(sx + sw / 2f + dx, sy + 30f + dy)
            }
            
            val r = spec.radius
            positionCircle(btn, center.first, center.second, r)
            
            // Position label inside button
            val labelSize = (r * 2).toInt()
            label.layout(
                (center.first - r).toInt(),
                (center.second - r).toInt(),
                (center.first + r).toInt(),
                (center.second + r).toInt()
            )
        }

        // Position stick
        stickHome = Pair(sx + 40f + STICK_RADIUS, sy + sh - 30f - STICK_RADIUS)
        positionCircle(stickBase!!, stickHome.first, stickHome.second, STICK_RADIUS)
        positionCircle(stickKnob!!, stickHome.first, stickHome.second, KNOB_RADIUS)
    }

    private fun positionCircle(view: View, cx: Float, cy: Float, radius: Float) {
        view.layout(
            (cx - radius).toInt(),
            (cy - radius).toInt(),
            (cx + radius).toInt(),
            (cy + radius).toInt()
        )
    }

    private fun relabel() {
        for ((i, spec) in buttonSpecs.withIndex()) {
            val (_, label) = buttonViews[i]!!
            val action = if (inCar) spec.inCarAction else spec.onFootAction
            val show = !spec.isNav || navMode
            label.visibility = if (show) View.VISIBLE else View.GONE
            buttonViews[i]?.first?.visibility = if (show) View.VISIBLE else View.GONE
            action?.let { label.text = spec.inCarLabel ?: spec.onFootLabel ?: "" }
        }
        restyle()
    }

    private fun restyle() {
        // Update button alphas based on pressed state
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (hidden) return false

        val action = event.actionMasked
        val pointerIndex = event.actionIndex
        val pointerId = event.getPointerId(pointerIndex)
        val x = event.getX(pointerIndex)
        val y = event.getY(pointerIndex)

        when (action) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                handleTouchDown(pointerId, x, y)
            }
            MotionEvent.ACTION_MOVE -> {
                for (i in 0 until event.pointerCount) {
                    val id = event.getPointerId(i)
                    handleTouchMove(id, event.getX(i), event.getY(i))
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL -> {
                handleTouchUp(pointerId)
            }
        }

        // Also forward to native
        nativeTouch(pointerId.toLong(), x, y, action)
        return true
    }

    private fun handleTouchDown(pointerId: Int, x: Float, y: Float) {
        val p = Pair(x / scale, y / scale)
        
        // Check buttons
        val btnIdx = findButtonAt(p)
        if (btnIdx != -1) {
            val spec = buttonSpecs[btnIdx]
            val action = if (inCar) spec.inCarAction else spec.onFootAction
            grips[pointerId] = Grip(GripType.Button, btnIdx, start = p, last = p, origin = p, current = p)
            
            action?.let {
                when (it) {
                    is Action.AimToggle -> aimLatched = !aimLatched
                    is Action.NavToggle -> { navMode = !navMode; relabel() }
                    is Action.KeyNav -> { if (!navMode) { navMode = true; relabel() } }
                }
            }
        } else if (inStickZone(p) && !grips.values.any { it.type == GripType.Stick }) {
            grips[pointerId] = Grip(GripType.Stick, origin = p, current = p)
        } else {
            grips[pointerId] = Grip(GripType.Look, last = p)
        }
        apply()
    }

    private fun handleTouchMove(pointerId: Int, x: Float, y: Float) {
        val p = Pair(x / scale, y / scale)
        val grip = grips[pointerId] ?: return
        
        val isWheel = grip.type == GripType.Button && isWheel(grip.index)
        
        when (grip.type) {
            GripType.Stick -> grip.current = p
            GripType.Button -> if (isWheel) grip.last = p
            GripType.Look -> {
                val (dx, dy) = Pair(p.first - grip.last.first, p.second - grip.last.second)
                grip.last = p
                val speed = Math.hypot(dx.toDouble(), dy.toDouble())
                val gain = 3.0 * touchSens * (1.0 + (speed / 12.0).coerceAtMost(1.5))
                inputMouseDelta((dx * gain).toFloat(), (dy * gain).toFloat())
            }
        }
        apply()
    }

    private fun handleTouchUp(pointerId: Int) {
        grips.remove(pointerId)
        apply()
    }

    private fun findButtonAt(p: Pair<Float, Float>): Int {
        for ((i, spec) in buttonSpecs.withIndex()) {
            if (spec.isNav && !navMode) continue
            val action = if (inCar) spec.inCarAction else spec.onFootAction
            if (action == null) continue
            
            val (btn, _) = buttonViews[i]!!
            val cx = (btn.left + btn.right) / 2f
            val cy = (btn.top + btn.bottom) / 2f
            val r = spec.radius + 8f
            if (Math.hypot((p.first - cx).toDouble(), (p.second - cy).toDouble()) <= r) {
                return i
            }
        }
        return -1
    }

    private fun inStickZone(p: Pair<Float, Float>): Boolean {
        return p.first < safeArea.left + safeArea.width() * 0.4f && p.second > safeArea.top + 70f
    }

    private fun isWheel(idx: Int): Boolean {
        val spec = buttonSpecs[idx]
        val action = if (inCar) spec.inCarAction else spec.onFootAction
        return when (action) {
            Action.Key("Tab") -> true
            Action.Key("AltLeft") -> true
            Action.Key("KeyQ") -> inCar
            else -> false
        }
    }

    private fun apply() {
        val keys = mutableSetOf<String>()
        var mouse = if (aimLatched && !inCar) 2 else 0
        var stickGrip: Grip? = null
        var horn = false

        for (grip in grips.values) {
            when (grip.type) {
                GripType.Stick -> stickGrip = grip
                GripType.Button -> {
                    val spec = buttonSpecs[grip.index]
                    val action = if (inCar) spec.inCarAction else spec.onFootAction
                    action?.let {
                        when (it) {
                            is Action.Key, is Action.KeyNav -> keys.add((it as Action.Key).component1())
                            is Action.Mouse -> mouse = mouse or it.component1()
                            is Action.Horn -> horn = true
                        }
                    }
                }
                GripType.Look -> {}
            }
        }
        
        stickGrip?.let { g ->
            val (dx, dy) = Pair(g.current.first - g.origin.first, g.current.second - g.origin.second)
            val len = Math.hypot(dx.toDouble(), dy.toDouble())
            val k = if (len > STICK_RADIUS) STICK_RADIUS / len else 1.0
            val ndx = dx * k
            val ndy = dy * k
            val nx = ndx / STICK_RADIUS
            val ny = ndy / STICK_RADIUS
            
            if (nx < -0.35) keys.add("KeyA")
            if (nx > 0.35) keys.add("KeyD")
            if (inCar) {
                if (ny < -0.6) keys.add("Numpad8")
                if (ny > 0.6) keys.add("Numpad2")
            } else {
                if (ny < -0.35) keys.add("KeyW")
                if (ny > 0.35) keys.add("KeyS")
                if (len > STICK_RADIUS * 0.95) keys.add("ShiftLeft")
            }
            
            // Update stick visual position
            positionCircle(stickBase!!, g.origin.first, g.origin.second, STICK_RADIUS)
            positionCircle(stickKnob!!, g.origin.first + ndx.toFloat(), g.origin.second + ndy.toFloat(), KNOB_RADIUS)
        } ?: run {
            positionCircle(stickBase!!, stickHome.first, stickHome.second, STICK_RADIUS)
            positionCircle(stickKnob!!, stickHome.first, stickHome.second, KNOB_RADIUS)
        }

        // Handle horn
        // pad.HORN_TOUCH.set(horn) - would need JNI bridge

        // Key press/release with hold logic
        val now = System.currentTimeMillis()
        val MIN_HOLD_MS = 120L

        for (k in heldKeys.toList()) {
            if (keys.contains(k)) continue
            val since = keySince[k] ?: 0L
            if (now - since < MIN_HOLD_MS) {
                keys.add(k)
                lingering = true
                continue
            }
            inputKey(k, false)
            keySince.remove(k)
        }

        for (k in keys - heldKeys) {
            mpKeyPressed(k)
            inputKey(k, true)
            keySince[k] = now
        }

        heldKeys.clear()
        heldKeys.addAll(keys)

        // Mouse buttons with hold
        for ((i, pair) in listOf(1 to 0, 2 to 2).withIndex()) {
            val (bit, dom) = pair
            if (mouseButtons and bit != 0 && mouse and bit == 0 && now - mouseSince[i] < MIN_HOLD_MS) {
                mouse = mouse or bit
                lingering = true
            }
            if ((mouse xor mouseButtons) and bit != 0) {
                inputButton(dom, (mouse and bit) != 0)
                if (mouse and bit != 0) mouseSince[i] = now
            }
        }
        mouseButtons = mouse
    }

    fun setSafeArea(rect: android.graphics.Rect) {
        safeArea = rect
        requestLayout()
    }

    fun update(hide: Boolean, inCar: Boolean) {
        nativeUpdate(hide, inCar)
        this.hidden = hide
        this.inCar = inCar
        visibility = if (hide) View.GONE else View.VISIBLE
        if (hide) {
            grips.clear()
            aimLatched = false
            apply()
        }
        if (inCar != this.inCar && !hidden) {
            grips.values.removeAll { it.type == GripType.Button }
            this.inCar = inCar
            if (inCar) aimLatched = false
            apply()
            relabel()
        }
    }
}