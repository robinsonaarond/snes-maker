; Shared SNES runtime bootstrap for the initial milestone.
; It draws the demo stage, spawns a simple player sprite, and supports
; basic Mega Man-style running, jumping, camera follow, and one bullet.

.setcpu "65816"
.smart on
.a8
.i16

.segment "ZEROPAGE"
joypad_high:      .res 1
prev_joypad_high: .res 1
tmp_screen_x:     .res 2
sprite_x:         .res 1
sprite_y:         .res 1
sprite_tile:      .res 1
sprite_attr:      .res 1

.segment "BSS"
camera_x:         .res 2
player_x:         .res 2
player_y:         .res 2
player_vy:        .res 2
bullet_x:         .res 2
bullet_y:         .res 2
player_facing:    .res 1 ; 0 = right, 1 = left
player_on_ground: .res 1
bullet_active:    .res 1
bullet_dir:       .res 1 ; 0 = right, 1 = left
oam_buffer:       .res $0220

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
    jsr InitOamBuffer
    jsr UpdateCamera
    jsr UpdateSprites
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
    jsr UpdatePlayer
    jsr UpdateBullet
    jsr UpdateCamera
    jsr UpdateSprites
    jmp MainLoop

InitPPU:
    lda #$01
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
    rep #$20
.a16
    lda #PROJECT_PLAYER_START_X
    sta player_x
    lda #PROJECT_PLAYER_START_Y
    sta player_y
    stz player_vy
    stz camera_x
    stz bullet_x
    stz bullet_y
    sep #$20
.a8
    stz player_facing
    stz prev_joypad_high
    stz joypad_high
    stz bullet_active
    stz bullet_dir
    lda #$01
    sta player_on_ground
    rts

InitOamBuffer:
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

UpdateBullet:
    lda joypad_high
    and #$40
    beq @move_bullet
    lda prev_joypad_high
    and #$40
    bne @move_bullet
    lda bullet_active
    bne @move_bullet
    lda #$01
    sta bullet_active
    lda player_facing
    sta bullet_dir
    lda player_facing
    bne @spawn_left
    rep #$20
.a16
    lda player_x
    clc
    adc #14
    sta bullet_x
    bra @spawn_y

@spawn_left:
    rep #$20
.a16
    lda player_x
    cmp #4
    bcs @subtract_left
    lda #0
    bra @store_spawn_left
@subtract_left:
    sec
    sbc #4
@store_spawn_left:
    sta bullet_x

@spawn_y:
    lda player_y
    clc
    adc #6
    sta bullet_y
    sep #$20
.a8

@move_bullet:
    lda bullet_active
    beq @done
    lda bullet_dir
    beq @move_right

    rep #$20
.a16
    lda bullet_x
    cmp #4
    bcc @hide
    sec
    sbc #4
    sta bullet_x
    sep #$20
.a8
    rts

@move_right:
    rep #$20
.a16
    lda bullet_x
    clc
    adc #4
    cmp #PROJECT_WORLD_WIDTH_PIXELS
    bcc @store_right_bullet
@hide:
    sep #$20
.a8
    stz bullet_active
    rts

@store_right_bullet:
    sta bullet_x
    sep #$20
.a8
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

UpdateSprites:
    ldx #0
    jsr HideSpriteAtX
    ldx #4
    jsr HideSpriteAtX
    stz oam_buffer+$0200

    rep #$20
.a16
    lda player_x
    sec
    sbc camera_x
    bmi @player_hidden
    cmp #256
    bcs @player_hidden
    sta tmp_screen_x
    sep #$20
.a8

    lda tmp_screen_x
    sta sprite_x
    lda player_y
    sta sprite_y
    lda #PROJECT_PLAYER_BASE_TILE
    sta sprite_tile
    lda player_facing
    beq @player_right
    lda #$70
    bra @store_player

@player_right:
    lda #$30
@store_player:
    sta sprite_attr
    ldx #0
    jsr WriteSpriteAtX
    lda #$02
    sta oam_buffer+$0200
    bra @bullet

@player_hidden:
    sep #$20
.a8

@bullet:
    lda bullet_active
    beq @done
    rep #$20
.a16
    lda bullet_x
    sec
    sbc camera_x
    bmi @done_16
    cmp #256
    bcs @done_16
    sta tmp_screen_x
    sep #$20
.a8
    lda tmp_screen_x
    sta sprite_x
    lda bullet_y
    sta sprite_y
    lda #PROJECT_BULLET_TILE
    sta sprite_tile
    lda #$30
    sta sprite_attr
    ldx #4
    jsr WriteSpriteAtX
    rts

@done_16:
    sep #$20
.a8
@done:
    rts

WriteSpriteAtX:
    lda sprite_x
    sta oam_buffer, x
    inx
    lda sprite_y
    sta oam_buffer, x
    inx
    lda sprite_tile
    sta oam_buffer, x
    inx
    lda sprite_attr
    sta oam_buffer, x
    rts

HideSpriteAtX:
    stz oam_buffer, x
    inx
    lda #$F0
    sta oam_buffer, x
    inx
    stz oam_buffer, x
    inx
    stz oam_buffer, x
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
