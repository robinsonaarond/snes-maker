; Shared SNES runtime bootstrap for the demo milestone.
; It renders the stage plus a simple object table with enemies, pickups,
; switches, solid props, a selectable health HUD, and up to three shots.

.setcpu "65816"
.smart on
.a8
.i16

MAX_RUNTIME_ENTITIES = 16
MAX_BULLETS = 3
PLAYER_WIDTH = 16
PLAYER_HEIGHT = 16
ACTION_NONE = 0
ACTION_HEAL_PLAYER = 1
ACTION_SET_ENTITY_ACTIVE = 2
KIND_PROP = 0
KIND_PICKUP = 1
KIND_ENEMY = 2
KIND_SWITCH = 3
KIND_SOLID = 4
MOVEMENT_NONE = 0
MOVEMENT_PATROL = 1

.segment "ZEROPAGE"
joypad_high:         .res 1
prev_joypad_high:    .res 1
frame_counter:       .res 1
draw_visual_index:   .res 1
draw_piece_start:    .res 1
draw_piece_count:    .res 1
draw_visual_width:   .res 1
draw_facing:         .res 1
draw_base_x:         .res 2
draw_base_y:         .res 2
piece_x:             .res 1
piece_y:             .res 1
piece_attr:          .res 1
sprite_x:            .res 1
sprite_y:            .res 1
sprite_tile:         .res 1
sprite_attr:         .res 1
oam_next:            .res 2
temp16:              .res 2
temp16_b:            .res 2
temp16_c:            .res 2
rect_left:           .res 2
rect_top:            .res 2
rect_right:          .res 2
rect_bottom:         .res 2

.segment "BSS"
camera_x:            .res 2
player_x:            .res 2
player_prev_x:       .res 2
player_y:            .res 2
player_vy:           .res 2
player_facing:       .res 1
player_on_ground:    .res 1
player_health:       .res 1
player_invuln:       .res 1

bullet_x_lo:         .res MAX_BULLETS
bullet_x_hi:         .res MAX_BULLETS
bullet_y_lo:         .res MAX_BULLETS
bullet_y_hi:         .res MAX_BULLETS
bullet_dir:          .res MAX_BULLETS
bullet_active:       .res MAX_BULLETS

entity_kind:         .res MAX_RUNTIME_ENTITIES
entity_flags:        .res MAX_RUNTIME_ENTITIES
entity_visual:       .res MAX_RUNTIME_ENTITIES
entity_facing:       .res MAX_RUNTIME_ENTITIES
entity_hitbox_x:     .res MAX_RUNTIME_ENTITIES
entity_hitbox_y:     .res MAX_RUNTIME_ENTITIES
entity_hitbox_w:     .res MAX_RUNTIME_ENTITIES
entity_hitbox_h:     .res MAX_RUNTIME_ENTITIES
entity_contact:      .res MAX_RUNTIME_ENTITIES
entity_hp:           .res MAX_RUNTIME_ENTITIES
entity_action_kind:  .res MAX_RUNTIME_ENTITIES
entity_action_value: .res MAX_RUNTIME_ENTITIES
entity_action_target:.res MAX_RUNTIME_ENTITIES
entity_move_kind:    .res MAX_RUNTIME_ENTITIES
entity_move_speed:   .res MAX_RUNTIME_ENTITIES
entity_x_lo:         .res MAX_RUNTIME_ENTITIES
entity_x_hi:         .res MAX_RUNTIME_ENTITIES
entity_y_lo:         .res MAX_RUNTIME_ENTITIES
entity_y_hi:         .res MAX_RUNTIME_ENTITIES
entity_patrol_min_lo:.res MAX_RUNTIME_ENTITIES
entity_patrol_min_hi:.res MAX_RUNTIME_ENTITIES
entity_patrol_max_lo:.res MAX_RUNTIME_ENTITIES
entity_patrol_max_hi:.res MAX_RUNTIME_ENTITIES

oam_buffer:          .res $0220

.segment "CODE"
Reset:
    sei
    clc
    xce
    rep #$10
    sep #$20
    ldx #$1FFF
    txs
    stz $4200
    stz $420C
    lda #$80
    sta $2100
    jsr InitPPU
    jsr LoadPalette
    jsr LoadObjPalette
    jsr LoadTiles
    jsr LoadObjTiles
    jsr LoadTilemap
    jsr InitGameState
    jsr ClearOamBuffer
    jsr UpdateCamera
    jsr RenderFrame
    jsr ApplyScroll
    jsr UploadOam
    lda #$11
    sta $212C
    lda #$81
    sta $4200
    lda #$0F
    sta $2100

MainLoop:
    jsr WaitForVBlank
    jsr UploadOam
    jsr ApplyScroll
    lda joypad_high
    sta prev_joypad_high
    jsr ReadJoypad
    inc frame_counter
    lda player_invuln
    beq @no_invuln_tick
    dec player_invuln
@no_invuln_tick:
    jsr UpdatePlayer
    jsr ResolveSolidCollisions
    jsr UpdateEnemies
    jsr UpdateBullets
    jsr UpdateEntityInteractions
    jsr UpdateCamera
    jsr RenderFrame
    jmp MainLoop

InitPPU:
    lda #$00
    sta $2101
    lda #$01
    sta $2105
    lda #$09
    sta $2107
    stz $2108
    stz $2109
    stz $210A
    stz $210B
    stz $210C
    stz $212C
    stz $212D
    stz $210D
    stz $210D
    stz $210E
    stz $210E
    stz $2102
    stz $2103
    rts

InitGameState:
    stz frame_counter
    stz joypad_high
    stz prev_joypad_high
    stz camera_x
    stz camera_x+1
    jsr ResetPlayerState
    jsr ResetBullets
    jsr LoadEntityState
    rts

ResetPlayerState:
    rep #$20
.a16
    lda #PROJECT_PLAYER_START_X
    sta player_x
    sta player_prev_x
    lda #PROJECT_PLAYER_START_Y
    sta player_y
    stz player_vy
    sep #$20
.a8
    stz player_facing
    lda #$01
    sta player_on_ground
    lda #PROJECT_PLAYER_STARTING_HEALTH
    sta player_health
    stz player_invuln
    rts

ResetBullets:
    ldx #0
@loop:
    stz bullet_x_lo, x
    stz bullet_x_hi, x
    stz bullet_y_lo, x
    stz bullet_y_hi, x
    stz bullet_dir, x
    stz bullet_active, x
    inx
    cpx #MAX_BULLETS
    bcc @loop
    rts

LoadEntityState:
    ldx #0
    ldy #0
@copy_entity:
    cpx #PROJECT_ENTITY_COUNT
    bcc @copy_entity_body
    jmp @clear_rest
@copy_entity_body:
    lda PROJECT_ENTITY_BYTES, y
    sta entity_kind, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_flags, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_visual, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_facing, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_hitbox_x, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_hitbox_y, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_hitbox_w, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_hitbox_h, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_contact, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_hp, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_action_kind, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_action_value, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_action_target, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_move_kind, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_move_speed, x
    iny
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_x_lo, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_x_hi, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_y_lo, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_y_hi, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_patrol_min_lo, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_patrol_min_hi, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_patrol_max_lo, x
    iny
    lda PROJECT_ENTITY_BYTES, y
    sta entity_patrol_max_hi, x
    iny
    inx
    jmp @copy_entity

@clear_rest:
    cpx #MAX_RUNTIME_ENTITIES
    bcs @done
    stz entity_kind, x
    stz entity_flags, x
    stz entity_visual, x
    stz entity_facing, x
    stz entity_hitbox_x, x
    stz entity_hitbox_y, x
    stz entity_hitbox_w, x
    stz entity_hitbox_h, x
    stz entity_contact, x
    stz entity_hp, x
    stz entity_action_kind, x
    stz entity_action_value, x
    lda #$FF
    sta entity_action_target, x
    stz entity_move_kind, x
    stz entity_move_speed, x
    stz entity_x_lo, x
    stz entity_x_hi, x
    stz entity_y_lo, x
    stz entity_y_hi, x
    stz entity_patrol_min_lo, x
    stz entity_patrol_min_hi, x
    stz entity_patrol_max_lo, x
    stz entity_patrol_max_hi, x
    inx
    bra @clear_rest
@done:
    rts

WaitForVBlank:
    wai
@wait_autojoy:
    lda $4212
    and #$01
    bne @wait_autojoy
    rts

ReadJoypad:
    lda $4219
    sta joypad_high
    rts

UpdatePlayer:
    rep #$20
.a16
    lda player_x
    sta player_prev_x
    sep #$20
.a8

    lda joypad_high
    and #$01
    beq @check_left
    lda joypad_high
    and #$02
    bne @check_jump
    rep #$20
.a16
    lda player_x
    cmp #PROJECT_PLAYER_MAX_X
    bcs @done_right
    clc
    adc #2
    cmp #(PROJECT_PLAYER_MAX_X + 1)
    bcc @store_right
    lda #PROJECT_PLAYER_MAX_X
@store_right:
    sta player_x
@done_right:
    sep #$20
.a8
    stz player_facing

@check_left:
    lda joypad_high
    and #$02
    beq @check_jump
    lda joypad_high
    and #$01
    bne @check_jump
    rep #$20
.a16
    lda player_x
    beq @done_left
    sec
    sbc #2
    bcs @store_left
    lda #0
@store_left:
    sta player_x
@done_left:
    sep #$20
.a8
    lda #$01
    sta player_facing

@check_jump:
    lda joypad_high
    and #$80
    beq @apply_physics
    lda prev_joypad_high
    and #$80
    bne @apply_physics
    lda player_on_ground
    beq @apply_physics
    rep #$20
.a16
    lda #$FFF7
    sta player_vy
    sep #$20
.a8
    stz player_on_ground

@apply_physics:
    lda player_on_ground
    bne @done
    rep #$20
.a16
    lda player_vy
    clc
    adc #1
    bmi @store_vy
    cmp #9
    bcc @store_vy
    lda #8
@store_vy:
    sta player_vy
    lda player_y
    clc
    adc player_vy
    sta player_y
    cmp #PROJECT_PLAYER_GROUND_Y
    bcc @still_airborne
    lda #PROJECT_PLAYER_GROUND_Y
    sta player_y
    stz player_vy
    sep #$20
.a8
    lda #$01
    sta player_on_ground
    rts

@still_airborne:
    sep #$20
.a8
    stz player_on_ground
@done:
    rts

ResolveSolidCollisions:
    ldx #0
@loop:
    cpx #PROJECT_ENTITY_COUNT
    bcs @done
    lda entity_flags, x
    and #$01
    beq @next
    lda entity_kind, x
    cmp #KIND_SOLID
    bne @next
    jsr BuildEntityRect
    jsr PlayerIntersectsRect
    bcc @next
    rep #$20
.a16
    lda player_x
    cmp player_prev_x
    beq @restore_done
    bcc @moving_left
    lda rect_left
    sec
    sbc #PLAYER_WIDTH
    sta player_x
    bra @restore_done
@moving_left:
    lda rect_right
    sta player_x
@restore_done:
    sep #$20
.a8
@next:
    inx
    bra @loop
@done:
    rts

UpdateEnemies:
    ldx #0
@loop:
    cpx #PROJECT_ENTITY_COUNT
    bcs @done
    lda entity_flags, x
    and #$01
    beq @next
    lda entity_kind, x
    cmp #KIND_ENEMY
    bne @next
    lda entity_move_kind, x
    cmp #MOVEMENT_PATROL
    bne @next
    lda entity_facing, x
    bne @move_left

    lda entity_x_lo, x
    clc
    adc entity_move_speed, x
    sta entity_x_lo, x
    lda entity_x_hi, x
    adc #0
    sta entity_x_hi, x
    jsr CompareEntityXToPatrolMax
    bcc @next
    lda entity_patrol_max_lo, x
    sta entity_x_lo, x
    lda entity_patrol_max_hi, x
    sta entity_x_hi, x
    lda #$01
    sta entity_facing, x
    bra @next

@move_left:
    lda entity_x_lo, x
    sec
    sbc entity_move_speed, x
    sta entity_x_lo, x
    lda entity_x_hi, x
    sbc #0
    sta entity_x_hi, x
    jsr CompareEntityXToPatrolMin
    bcs @next
    lda entity_patrol_min_lo, x
    sta entity_x_lo, x
    lda entity_patrol_min_hi, x
    sta entity_x_hi, x
    stz entity_facing, x
@next:
    inx
    bra @loop
@done:
    rts

UpdateBullets:
    lda joypad_high
    and #$40
    beq @move
    lda prev_joypad_high
    and #$40
    bne @move
    jsr SpawnBullet

@move:
    ldx #0
@loop:
    cpx #MAX_BULLETS
    bcs @done
    lda bullet_active, x
    beq @next
    lda bullet_dir, x
    beq @move_right

    lda bullet_x_lo, x
    cmp #4
    bcc @deactivate
    sec
    sbc #4
    sta bullet_x_lo, x
    lda bullet_x_hi, x
    sbc #0
    sta bullet_x_hi, x
    bra @check_collisions

@move_right:
    lda bullet_x_lo, x
    clc
    adc #4
    sta bullet_x_lo, x
    lda bullet_x_hi, x
    adc #0
    sta bullet_x_hi, x
    lda bullet_x_hi, x
    cmp #>(PROJECT_WORLD_WIDTH_PIXELS)
    bcc @check_collisions
    bne @deactivate
    lda bullet_x_lo, x
    cmp #<(PROJECT_WORLD_WIDTH_PIXELS)
    bcc @check_collisions
    bra @deactivate

@check_collisions:
    jsr CheckBulletAgainstEntities
    bra @next

@deactivate:
    stz bullet_active, x
@next:
    inx
    bra @loop
@done:
    rts

SpawnBullet:
    ldx #0
@find_slot:
    cpx #MAX_BULLETS
    bcs @done
    lda bullet_active, x
    beq @spawn_here
    inx
    bra @find_slot

@spawn_here:
    lda #$01
    sta bullet_active, x
    lda player_facing
    sta bullet_dir, x
    rep #$20
.a16
    lda player_x
    clc
    adc #14
    sta temp16
    lda player_y
    clc
    adc #6
    sta temp16_b
    sep #$20
.a8
    lda player_facing
    beq @store
    lda temp16
    cmp #4
    bcs @shift_left
    lda #0
    sta temp16
    bra @store
@shift_left:
    rep #$20
.a16
    lda temp16
    sec
    sbc #4
    sta temp16
    sep #$20
.a8
@store:
    lda temp16
    sta bullet_x_lo, x
    lda temp16+1
    sta bullet_x_hi, x
    lda temp16_b
    sta bullet_y_lo, x
    lda temp16_b+1
    sta bullet_y_hi, x
@done:
    rts

UpdateEntityInteractions:
    ldx #0
@loop:
    cpx #PROJECT_ENTITY_COUNT
    bcs @done
    lda entity_flags, x
    and #$01
    beq @next
    jsr BuildEntityRect
    lda entity_kind, x
    cmp #KIND_ENEMY
    bne @check_pickup
    lda player_invuln
    bne @next
    jsr PlayerIntersectsRect
    bcc @next
    lda entity_contact, x
    beq @next
    jsr DamagePlayer
    bra @next

@check_pickup:
    cmp #KIND_PICKUP
    beq @activate_pickup
    cmp #KIND_SWITCH
    bne @next
@activate_switch:
    jsr PlayerIntersectsRect
    bcc @next
    jsr ApplyEntityAction
    lda entity_flags, x
    and #$02
    beq @next
    lda entity_flags, x
    and #$FE
    sta entity_flags, x
    bra @next
@activate_pickup:
    jsr PlayerIntersectsRect
    bcc @next
    jsr ApplyEntityAction
    lda entity_flags, x
    and #$FE
    sta entity_flags, x
@next:
    inx
    bra @loop
@done:
    rts

DamagePlayer:
    sta temp16
    lda player_health
    sec
    sbc temp16
    bcs @store
    lda #0
@store:
    sta player_health
    lda #45
    sta player_invuln
    lda player_health
    bne @done
    jsr ResetPlayerState
@done:
    rts

ApplyEntityAction:
    lda entity_action_kind, x
    beq @done
    cmp #ACTION_HEAL_PLAYER
    bne @check_toggle
    lda player_health
    clc
    adc entity_action_value, x
    cmp #PROJECT_PLAYER_MAX_HEALTH
    bcc @store_heal
    lda #PROJECT_PLAYER_MAX_HEALTH
@store_heal:
    sta player_health
    rts

@check_toggle:
    cmp #ACTION_SET_ENTITY_ACTIVE
    bne @done
    lda entity_action_target, x
    cmp #$FF
    beq @done
    sta temp16
    stz temp16+1
    ldy temp16
    lda entity_flags, y
    and #$FE
    sta entity_flags, y
    lda entity_action_value, x
    beq @done
    lda entity_flags, y
    ora #$01
    sta entity_flags, y
@done:
    rts

UpdateCamera:
    rep #$20
.a16
    lda player_x
    sec
    sbc #120
    bmi @clamp_left
    cmp #PROJECT_MAX_SCROLL_X
    bcc @store
    lda #PROJECT_MAX_SCROLL_X
    bra @store
@clamp_left:
    lda #0
@store:
    sta camera_x
    sep #$20
.a8
    rts

ApplyScroll:
    lda camera_x
    sta $210D
    lda camera_x+1
    sta $210D
    stz $210E
    stz $210E
    rts

LoadPalette:
    stz $2121
    ldx #$0000
@loop:
    cpx #PROJECT_BG_PALETTE_BYTE_LEN
    bcs @done
    lda PROJECT_BG_PALETTE, x
    sta $2122
    inx
    bra @loop
@done:
    rts

LoadObjPalette:
    lda #$80
    sta $2121
    ldx #$0000
@loop:
    cpx #PROJECT_OBJ_PALETTE_BYTE_LEN
    bcs @done
    lda PROJECT_OBJ_PALETTE, x
    sta $2122
    inx
    bra @loop
@done:
    rts

LoadTiles:
    lda #$80
    sta $2115
    stz $2116
    stz $2117
    ldx #$0000
@loop:
    cpx #PROJECT_BG_TILE_BYTE_LEN
    bcs @done
    lda PROJECT_BG_TILE_BYTES, x
    sta $2118
    inx
    cpx #PROJECT_BG_TILE_BYTE_LEN
    bcs @done
    lda PROJECT_BG_TILE_BYTES, x
    sta $2119
    inx
    bra @loop
@done:
    rts

LoadObjTiles:
    lda #$80
    sta $2115
    stz $2116
    lda #$20
    sta $2117
    ldx #$0000
@loop:
    cpx #PROJECT_OBJ_TILE_BYTE_LEN
    bcs @done
    lda PROJECT_OBJ_TILE_BYTES, x
    sta $2118
    inx
    cpx #PROJECT_OBJ_TILE_BYTE_LEN
    bcs @done
    lda PROJECT_OBJ_TILE_BYTES, x
    sta $2119
    inx
    bra @loop
@done:
    rts

LoadTilemap:
    lda #$80
    sta $2115
    lda #$00
    sta $2116
    lda #$08
    sta $2117
    ldx #$0000
@loop:
    cpx #PROJECT_BG_MAP_BYTE_LEN
    bcs @done
    lda PROJECT_BG_MAP_BYTES, x
    sta $2118
    inx
    cpx #PROJECT_BG_MAP_BYTE_LEN
    bcs @done
    lda PROJECT_BG_MAP_BYTES, x
    sta $2119
    inx
    bra @loop
@done:
    rts

RenderFrame:
    jsr ClearOamBuffer
    jsr DrawPlayer
    jsr DrawBullets
    jsr DrawEntities
    jsr DrawHud
    rts

ClearOamBuffer:
    stz oam_next
    stz oam_next+1
    ldx #$0000
@clear_low:
    stz oam_buffer, x
    inx
    lda #$F0
    sta oam_buffer, x
    inx
    stz oam_buffer, x
    inx
    stz oam_buffer, x
    inx
    cpx #$0200
    bcc @clear_low
    ldx #$0200
@clear_high:
    stz oam_buffer, x
    inx
    cpx #$0220
    bcc @clear_high
    rts

DrawPlayer:
    rep #$20
.a16
    lda player_x
    sec
    sbc camera_x
    bmi @hidden
    cmp #256
    bcs @hidden
    sta draw_base_x
    lda player_y
    sta draw_base_y
    sep #$20
.a8
    lda #PROJECT_PLAYER_VISUAL
    sta draw_visual_index
    lda player_invuln
    beq @check_idle_blink
    lda frame_counter
    and #$02
    bne @visual_ready
    lda #PROJECT_PLAYER_ALT_VISUAL
    sta draw_visual_index
    bra @visual_ready
@check_idle_blink:
    lda joypad_high
    and #$03
    bne @visual_ready
    lda player_on_ground
    beq @visual_ready
    lda frame_counter
    and #$1F
    cmp #$1D
    bcc @visual_ready
    lda #PROJECT_PLAYER_ALT_VISUAL
    sta draw_visual_index
@visual_ready:
    lda player_facing
    sta draw_facing
    jsr DrawVisualAtBase
    rts
@hidden:
    sep #$20
.a8
    rts

DrawBullets:
    ldx #0
@loop:
    cpx #MAX_BULLETS
    bcs @done
    lda bullet_active, x
    beq @next
    lda bullet_x_lo, x
    sta temp16
    lda bullet_x_hi, x
    sta temp16+1
    rep #$20
.a16
    lda temp16
    sec
    sbc camera_x
    bmi @skip
    cmp #256
    bcs @skip
    sta draw_base_x
    sep #$20
.a8
    lda bullet_y_lo, x
    sta draw_base_y
    lda bullet_y_hi, x
    sta draw_base_y+1
    lda #PROJECT_BULLET_VISUAL
    sta draw_visual_index
    lda bullet_dir, x
    sta draw_facing
    jsr DrawVisualAtBase
    bra @next
@skip:
    sep #$20
.a8
@next:
    inx
    bra @loop
@done:
    rts

DrawEntities:
    ldx #0
@loop:
    cpx #PROJECT_ENTITY_COUNT
    bcs @done
    lda entity_flags, x
    and #$01
    beq @next
    lda entity_x_lo, x
    sta temp16
    lda entity_x_hi, x
    sta temp16+1
    rep #$20
.a16
    lda temp16
    sec
    sbc camera_x
    bmi @skip
    cmp #256
    bcs @skip
    sta draw_base_x
    sep #$20
.a8
    lda entity_y_lo, x
    sta draw_base_y
    lda entity_y_hi, x
    sta draw_base_y+1
    lda entity_visual, x
    sta draw_visual_index
    lda entity_facing, x
    sta draw_facing
    jsr DrawVisualAtBase
    bra @next
@skip:
    sep #$20
.a8
@next:
    inx
    bra @loop
@done:
    rts

DrawHud:
    lda #PROJECT_HEALTH_HUD_STYLE
    beq @pips
    cmp #1
    beq @hearts
    jmp DrawCellsHud

@pips:
    ldx #0
@pip_loop:
    cpx #PROJECT_PLAYER_MAX_HEALTH
    bcs @done
    txa
    asl
    asl
    asl
    clc
    adc #8
    sta draw_base_x
    stz draw_base_x+1
    lda #8
    sta draw_base_y
    stz draw_base_y+1
    txa
    cmp player_health
    bcs @pip_empty
    lda #PROJECT_HUD_PIP_FULL_VISUAL
    bra @draw_pip
@pip_empty:
    lda #PROJECT_HUD_PIP_EMPTY_VISUAL
@draw_pip:
    sta draw_visual_index
    stz draw_facing
    jsr DrawVisualAtBase
    inx
    bra @pip_loop

@hearts:
    ldx #0
@heart_loop:
    cpx #PROJECT_PLAYER_MAX_HEALTH
    bcs @done
    txa
    asl
    asl
    asl
    sta temp16
    lda #240
    sec
    sbc temp16
    sta draw_base_x
    stz draw_base_x+1
    lda #8
    sta draw_base_y
    stz draw_base_y+1
    txa
    cmp player_health
    bcs @heart_empty
    lda #PROJECT_HUD_HEART_FULL_VISUAL
    bra @draw_heart
@heart_empty:
    lda #PROJECT_HUD_HEART_EMPTY_VISUAL
@draw_heart:
    sta draw_visual_index
    stz draw_facing
    jsr DrawVisualAtBase
    inx
    bra @heart_loop

@done:
    rts

DrawCellsHud:
    ldx #0
@cell_loop:
    cpx #PROJECT_PLAYER_MAX_HEALTH
    bcs @done
    txa
    asl
    asl
    asl
    clc
    adc #96
    sta draw_base_x
    stz draw_base_x+1
    lda #8
    sta draw_base_y
    stz draw_base_y+1
    txa
    cmp player_health
    bcs @cell_empty
    lda #PROJECT_HUD_CELL_FULL_VISUAL
    bra @draw_cell
@cell_empty:
    lda #PROJECT_HUD_CELL_EMPTY_VISUAL
@draw_cell:
    sta draw_visual_index
    stz draw_facing
    jsr DrawVisualAtBase
    inx
    bra @cell_loop
@done:
    rts

DrawVisualAtBase:
    lda draw_visual_index
    asl
    clc
    adc draw_visual_index
    tax
    lda PROJECT_VISUAL_HEADERS, x
    sta draw_piece_start
    inx
    lda PROJECT_VISUAL_HEADERS, x
    sta draw_piece_count
    inx
    lda PROJECT_VISUAL_HEADERS, x
    sta draw_visual_width
    lda draw_piece_count
    sta temp16_c
    stz temp16_c+1
    ldy #0
@loop:
    cpy temp16_c
    bcs @done
    lda draw_piece_start
    clc
    adc #0
    sta temp16
    tya
    clc
    adc temp16
    asl
    asl
    tax
    lda PROJECT_VISUAL_PIECES, x
    sta sprite_tile
    inx
    lda PROJECT_VISUAL_PIECES, x
    sta piece_x
    inx
    lda PROJECT_VISUAL_PIECES, x
    sta piece_y
    inx
    lda PROJECT_VISUAL_PIECES, x
    sta piece_attr

    lda draw_facing
    beq @facing_right
    lda draw_visual_width
    sec
    sbc #8
    sec
    sbc piece_x
    sta temp16
    bra @piece_x_ready
@facing_right:
    lda piece_x
    sta temp16
@piece_x_ready:
    lda draw_base_x
    clc
    adc temp16
    sta sprite_x
    lda draw_base_x+1
    adc #0
    bne @next_piece
    lda draw_base_y
    clc
    adc piece_y
    sta sprite_y
    lda draw_base_y+1
    adc #0
    bne @next_piece
    lda sprite_y
    cmp #$F0
    bcs @next_piece
    lda piece_attr
    sta sprite_attr
    lda draw_facing
    beq @append
    lda sprite_attr
    eor #$40
    sta sprite_attr
@append:
    jsr AppendSprite
@next_piece:
    iny
    bra @loop
@done:
    rts

AppendSprite:
    lda oam_next+1
    cmp #$02
    bcs @done
    ldy oam_next
    lda sprite_x
    sta oam_buffer, y
    iny
    lda sprite_y
    sta oam_buffer, y
    iny
    lda sprite_tile
    sta oam_buffer, y
    iny
    lda sprite_attr
    sta oam_buffer, y
    iny
    sty oam_next
@done:
    rts

UploadOam:
    stz $2102
    stz $2103
    stz $4300
    lda #$04
    sta $4301
    lda #<oam_buffer
    sta $4302
    lda #>oam_buffer
    sta $4303
    lda #^oam_buffer
    sta $4304
    lda #$20
    sta $4305
    lda #$02
    sta $4306
    lda #$01
    sta $420B
    rts

BuildEntityRect:
    lda entity_hitbox_x, x
    sta temp16_b
    bmi @neg_x
    stz temp16_b+1
    bra @hitbox_x_ready
@neg_x:
    lda #$FF
    sta temp16_b+1
@hitbox_x_ready:
    lda entity_x_lo, x
    sta temp16
    lda entity_x_hi, x
    sta temp16+1
    rep #$20
.a16
    lda temp16
    clc
    adc temp16_b
    sta rect_left
    sep #$20
.a8
    lda entity_hitbox_y, x
    sta temp16_b
    bmi @neg_y
    stz temp16_b+1
    bra @hitbox_y_ready
@neg_y:
    lda #$FF
    sta temp16_b+1
@hitbox_y_ready:
    lda entity_y_lo, x
    sta temp16
    lda entity_y_hi, x
    sta temp16+1
    rep #$20
.a16
    lda temp16
    clc
    adc temp16_b
    sta rect_top
    sep #$20
.a8
    lda rect_left
    clc
    adc entity_hitbox_w, x
    sta rect_right
    lda rect_left+1
    adc #0
    sta rect_right+1
    lda rect_top
    clc
    adc entity_hitbox_h, x
    sta rect_bottom
    lda rect_top+1
    adc #0
    sta rect_bottom+1
    rts

PlayerIntersectsRect:
    rep #$20
.a16
    lda player_x
    cmp rect_right
    bcs @no
    lda player_y
    cmp rect_bottom
    bcs @no
    lda player_x
    clc
    adc #PLAYER_WIDTH
    cmp rect_left
    beq @no
    bcc @no
    lda player_y
    clc
    adc #PLAYER_HEIGHT
    cmp rect_top
    beq @no
    bcc @no
    sep #$20
.a8
    sec
    rts
@no:
    sep #$20
.a8
    clc
    rts

BulletIntersectsRect:
    lda bullet_x_lo, x
    sta temp16
    lda bullet_x_hi, x
    sta temp16+1
    lda bullet_y_lo, x
    sta temp16_b
    lda bullet_y_hi, x
    sta temp16_b+1
    rep #$20
.a16
    lda temp16
    cmp rect_right
    bcs @no
    lda temp16_b
    cmp rect_bottom
    bcs @no
    lda temp16
    clc
    adc #8
    cmp rect_left
    beq @no
    bcc @no
    lda temp16_b
    clc
    adc #8
    cmp rect_top
    beq @no
    bcc @no
    sep #$20
.a8
    sec
    rts
@no:
    sep #$20
.a8
    clc
    rts

CheckBulletAgainstEntities:
    phx
    ldy #0
@loop:
    cpy #PROJECT_ENTITY_COUNT
    bcs @done
    lda entity_flags, y
    and #$01
    beq @next
    tya
    tax
    jsr BuildEntityRect
    plx
    phx
    jsr BulletIntersectsRect
    bcc @next
    tya
    tax
    lda entity_kind, x
    cmp #KIND_ENEMY
    beq @hit_enemy
    cmp #KIND_SOLID
    beq @hit_solid
    bra @next
@hit_enemy:
    lda entity_hp, x
    beq @deactivate_bullet
    dec entity_hp, x
    lda entity_hp, x
    bne @deactivate_bullet
    lda entity_flags, x
    and #$FE
    sta entity_flags, x
    bra @deactivate_bullet
@hit_solid:
@deactivate_bullet:
    plx
    stz bullet_active, x
    rts
@next:
    plx
    phx
    iny
    bra @loop
@done:
    plx
    rts

CompareEntityXToPatrolMax:
    lda entity_x_hi, x
    cmp entity_patrol_max_hi, x
    bne @done
    lda entity_x_lo, x
    cmp entity_patrol_max_lo, x
@done:
    rts

CompareEntityXToPatrolMin:
    lda entity_x_hi, x
    cmp entity_patrol_min_hi, x
    bne @done
    lda entity_x_lo, x
    cmp entity_patrol_min_lo, x
@done:
    rts

Nmi:
    rep #$20
    pha
    sep #$20
    lda $4210
    rep #$20
    pla
    rti

Irq:
    rti

.segment "RODATA"
.include "generated/project_data.inc"

.segment "HEADER"
.include "generated/header.inc"

.segment "VECTORS"
.word $0000
.word $0000
.word $0000
.word Nmi
.word $0000
.word Irq
.word $0000
.word $0000
.word $0000
.word Nmi
.word Reset
.word Irq
