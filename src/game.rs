use std::rc::Rc;

use crate::{
    browser,
    engine::{
        self, Cell, Game, GameLoop, Image, KeyState, Point, Rect, Renderer, Sheet, SpriteSheet,
    },
    segments::stone_and_platform,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use gloo_utils::format::JsValueSerdeExt;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use web_sys::HtmlImageElement;

const HEIGHT: i16 = 600;
const TIMELINE_MINIMUM: i16 = 1000;
const OBSTACLE_BUFFER: i16 = 20;

macro_rules! log {
    ( $( $t:tt )* ) => {
        web_sys::console::log_1(&format!( $( $t )* ).into());
    };
}

pub struct Walk {
    boy: RedHatBoy,
    background: [Image; 2],
    obstacles: Vec<Box<dyn Obstacle>>,
    obstacle_sheet: Rc<SpriteSheet>,
    stone: HtmlImageElement,
    timeline: i16,
}

impl Walk {
    fn velocity(&self) -> i16 {
        -self.boy.walk_speed()
    }
}

pub enum WalkTheDog {
    Loading,
    Loaded(Walk),
}

#[async_trait(?Send)]
impl Game for WalkTheDog {
    async fn initialize(&self) -> Result<Box<dyn Game>> {
        match self {
            WalkTheDog::Loading => {
                let tiles = browser::fetch_json("tiles.json").await?;
                let sprite_sheet = Rc::new(SpriteSheet::new(
                    tiles.into_serde::<Sheet>()?,
                    engine::load_image("tiles.png").await?,
                ));

                let json = browser::fetch_json("rhb_trimmed.json").await?;
                let rhb = RedHatBoy::new(
                    json.into_serde::<Sheet>()?,
                    engine::load_image("rhb_trimmed.png").await?,
                );

                let background = engine::load_image("BG.png").await?;
                let stone = engine::load_image("Stone.png").await?;
                let background_width = background.width() as i16;
                let starting_obstacles = stone_and_platform(stone.clone(), sprite_sheet.clone(), 0);
                let timeline = rightmost(&starting_obstacles);
                Ok(Box::new(WalkTheDog::Loaded(Walk {
                    boy: rhb,
                    background: [
                        Image::new(background.clone(), Point { x: 0, y: 0 }),
                        Image::new(
                            background,
                            Point {
                                x: background_width,
                                y: 0,
                            },
                        ),
                    ],
                    obstacles: starting_obstacles,
                    obstacle_sheet: sprite_sheet,
                    stone,
                    timeline,
                })))
            }
            WalkTheDog::Loaded(_) => Err(anyhow!("Error: Game is already initialized!")),
        }
    }

    fn update(&mut self, keystate: &KeyState) {
        if let WalkTheDog::Loaded(walk) = self {
            if keystate.is_pressed("ArrowRight") {
                walk.boy.run_right();
            }
            if keystate.is_pressed("ArrowDown") {
                walk.boy.slide();
            }
            if keystate.is_pressed("Space") {
                walk.boy.jump();
            }
            walk.boy.update();

            let velocity = walk.velocity();
            // 条件を満たす要素のみを残す
            walk.obstacles.retain(|obstacle| obstacle.right() > 0);
            walk.obstacles.iter_mut().for_each(|obstacle| {
                obstacle.move_horizontally(velocity);
                obstacle.check_intersection(&mut walk.boy);
            });

            let [first_background, second_background] = &mut walk.background;
            first_background.move_horizontally(velocity);
            second_background.move_horizontally(velocity);
            if first_background.right() < 0 {
                first_background.set_x(second_background.right());
            }
            if second_background.right() < 0 {
                second_background.set_x(first_background.right());
            }

            walk.obstacles.iter_mut().for_each(|obstacle| {
                obstacle.move_horizontally(velocity);
                obstacle.check_intersection(&mut walk.boy);
            });

            if walk.timeline < TIMELINE_MINIMUM {
                let mut next_obstacles = stone_and_platform(
                    walk.stone.clone(),
                    walk.obstacle_sheet.clone(),
                    walk.timeline + OBSTACLE_BUFFER,
                );

                walk.timeline = rightmost(&next_obstacles);
                walk.obstacles.append(&mut next_obstacles);
            } else {
                walk.timeline += velocity;
            }
        }
    }

    fn draw(&self, renderer: &Renderer) {
        renderer.clear(&Rect::new_from_x_y(0, 0, 600, 600));

        if let WalkTheDog::Loaded(walk) = self {
            walk.background.iter().for_each(|background| {
                background.draw(renderer);
            });
            walk.boy.draw(renderer);
            walk.obstacles.iter().for_each(|obstacle| {
                obstacle.draw(renderer);
            })
        }
    }
}
impl WalkTheDog {
    fn new() -> Self {
        WalkTheDog::Loading
    }
}

pub trait Obstacle {
    fn check_intersection(&self, boy: &mut RedHatBoy);
    fn draw(&self, renderer: &Renderer);
    fn move_horizontally(&mut self, x: i16);
    fn right(&self) -> i16;
}

fn rightmost(obstacle_list: &Vec<Box<dyn Obstacle>>) -> i16 {
    obstacle_list
        .iter()
        .map(|obstacle| obstacle.right())
        .max_by(|x, y| x.cmp(&y))
        .unwrap_or(0)
}

pub struct Platform {
    sheet: Rc<SpriteSheet>,
    position: Point,
    sprites: Vec<Cell>,
    bounding_boxes: Vec<Rect>,
}

impl Obstacle for Platform {
    fn check_intersection(&self, boy: &mut RedHatBoy) {
        if let Some(box_to_land_on) = self
            .bounding_boxes()
            .iter()
            .find(|&bounding_box| boy.bounding_box().intersects(bounding_box))
        {
            if boy.velocity_y() > 0 && boy.pos_y() < self.position.y {
                boy.land_on(box_to_land_on.y());
            } else {
                boy.knock_out();
            }
        }
    }

    fn draw(&self, renderer: &Renderer) {
        let mut x = 0;
        self.sprites.iter().for_each(|sprite| {
            self.sheet.draw(
                renderer,
                &Rect::new_from_x_y(
                    sprite.frame.x,
                    sprite.frame.y,
                    sprite.frame.w,
                    sprite.frame.h,
                ),
                &Rect::new_from_x_y(
                    self.position.x + x,
                    self.position.y,
                    sprite.frame.w,
                    sprite.frame.h,
                ),
            );
            x += sprite.frame.w;
        });
    }

    fn move_horizontally(&mut self, x: i16) {
        self.position.x += x;
        self.bounding_boxes.iter_mut().for_each(|bounding_box| {
            bounding_box.set_x(bounding_box.position.x + x);
        })
    }

    fn right(&self) -> i16 {
        self.bounding_boxes()
            .last()
            .unwrap_or(&Rect::default())
            .right()
    }
}

impl Platform {
    pub fn new(
        sheet: Rc<SpriteSheet>,
        position: Point,
        sprite_names: &[&str],
        bounding_boxes: &[Rect],
    ) -> Self {
        let sprites = sprite_names
            .iter()
            .filter_map(|sprite_name| sheet.cell(&sprite_name).cloned())
            .collect();
        let bounding_boxes = bounding_boxes
            .iter()
            .map(|bounding_box| {
                Rect::new_from_x_y(
                    bounding_box.x() + position.x,
                    bounding_box.y() + position.y,
                    bounding_box.w,
                    bounding_box.h,
                )
            })
            .collect();

        Platform {
            sheet,
            position,
            sprites,
            bounding_boxes,
        }
    }

    fn destination_box(&self) -> Rect {
        let platform = self.sheet.cell("13.png").expect("13.png does not exist");

        Rect::new(
            self.position,
            (platform.frame.w * 3).into(),
            platform.frame.h.into(),
        )
    }

    fn bounding_boxes(&self) -> &Vec<Rect> {
        &self.bounding_boxes
    }
}

pub struct Barrier {
    image: Image,
}

impl Obstacle for Barrier {
    fn check_intersection(&self, boy: &mut RedHatBoy) {
        if boy.bounding_box().intersects(self.image.bounding_box()) {
            boy.knock_out();
        }
    }

    fn draw(&self, renderer: &Renderer) {
        self.image.draw(renderer);
    }

    fn move_horizontally(&mut self, x: i16) {
        self.image.move_horizontally(x);
    }

    fn right(&self) -> i16 {
        self.image.right()
    }
}

impl Barrier {
    pub fn new(image: Image) -> Self {
        Barrier { image }
    }
}

#[derive(Copy, Clone)]
enum RedHatBoyStateMachine {
    Idle(RedHatBoyState<Idle>),
    Running(RedHatBoyState<Running>),
    Sliding(RedHatBoyState<Sliding>),
    Jumping(RedHatBoyState<Jumping>),
    Falling(RedHatBoyState<Falling>),
    KnockedOut(RedHatBoyState<KnockedOut>),
}
impl From<RedHatBoyState<Idle>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<Idle>) -> Self {
        RedHatBoyStateMachine::Idle(state)
    }
}
impl From<RedHatBoyState<Running>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<Running>) -> Self {
        RedHatBoyStateMachine::Running(state)
    }
}
impl From<RedHatBoyState<Sliding>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<Sliding>) -> Self {
        RedHatBoyStateMachine::Sliding(state)
    }
}
impl From<RedHatBoyState<Jumping>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<Jumping>) -> Self {
        RedHatBoyStateMachine::Jumping(state)
    }
}
impl From<SlidingEndState> for RedHatBoyStateMachine {
    fn from(end_state: SlidingEndState) -> Self {
        match end_state {
            SlidingEndState::Complete(running_state) => running_state.into(),
            SlidingEndState::Sliding(sliding_state) => sliding_state.into(),
        }
    }
}
impl From<JumpingEndState> for RedHatBoyStateMachine {
    fn from(end_state: JumpingEndState) -> Self {
        match end_state {
            JumpingEndState::Landing(running_state) => running_state.into(),
            JumpingEndState::Jumping(jumping_state) => jumping_state.into(),
        }
    }
}
impl From<RedHatBoyState<Falling>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<Falling>) -> Self {
        RedHatBoyStateMachine::Falling(state)
    }
}
impl From<RedHatBoyState<KnockedOut>> for RedHatBoyStateMachine {
    fn from(state: RedHatBoyState<KnockedOut>) -> Self {
        RedHatBoyStateMachine::KnockedOut(state)
    }
}
impl From<FallingEndState> for RedHatBoyStateMachine {
    fn from(end_state: FallingEndState) -> Self {
        match end_state {
            FallingEndState::Complete(knocked_out_state) => knocked_out_state.into(),
            FallingEndState::Falling(falling_state) => falling_state.into(),
        }
    }
}

pub enum Event {
    Run,
    Slide,
    Jump,
    KnockOut,
    Land(i16),
    Update,
}

impl RedHatBoyStateMachine {
    fn transition(self, event: Event) -> Self {
        match (self, event) {
            (RedHatBoyStateMachine::Idle(state), Event::Run) => state.run().into(),
            (RedHatBoyStateMachine::Running(state), Event::Slide) => state.slide().into(),
            (RedHatBoyStateMachine::Running(state), Event::Jump) => state.jump().into(),
            (RedHatBoyStateMachine::Running(state), Event::KnockOut) => state.knock_out().into(),
            (RedHatBoyStateMachine::Sliding(state), Event::KnockOut) => state.knock_out().into(),
            (RedHatBoyStateMachine::Jumping(state), Event::KnockOut) => state.knock_out().into(),
            (RedHatBoyStateMachine::Idle(state), Event::Update) => state.update().into(),
            (RedHatBoyStateMachine::Running(state), Event::Update) => state.update().into(),
            (RedHatBoyStateMachine::Sliding(state), Event::Update) => state.update().into(),
            (RedHatBoyStateMachine::Jumping(state), Event::Update) => state.update().into(),
            (RedHatBoyStateMachine::Falling(state), Event::Update) => state.update().into(),
            (RedHatBoyStateMachine::Jumping(state), Event::Land(position)) => {
                state.land_on(position).into()
            }
            (RedHatBoyStateMachine::Running(state), Event::Land(position)) => {
                state.land_on(position).into()
            }
            _ => self,
        }
    }

    fn frame_name(&self) -> &str {
        match self {
            RedHatBoyStateMachine::Idle(state) => state.frame_name(),
            RedHatBoyStateMachine::Running(state) => state.frame_name(),
            RedHatBoyStateMachine::Sliding(state) => state.frame_name(),
            RedHatBoyStateMachine::Jumping(state) => state.frame_name(),
            RedHatBoyStateMachine::Falling(state) => state.frame_name(),
            RedHatBoyStateMachine::KnockedOut(state) => state.frame_name(),
        }
    }

    fn context(&self) -> &RedHatBoyContext {
        match self {
            RedHatBoyStateMachine::Idle(state) => &state.context(),
            RedHatBoyStateMachine::Running(state) => &state.context(),
            RedHatBoyStateMachine::Sliding(state) => &state.context(),
            RedHatBoyStateMachine::Jumping(state) => &state.context(),
            RedHatBoyStateMachine::Falling(state) => &state.context(),
            RedHatBoyStateMachine::KnockedOut(state) => &state.context(),
        }
    }

    fn update(self) -> Self {
        self.transition(Event::Update)
    }
}

struct RedHatBoy {
    state_machine: RedHatBoyStateMachine,
    sprite_sheet: Sheet,
    image: HtmlImageElement,
}

impl RedHatBoy {
    fn new(sheet: Sheet, image: HtmlImageElement) -> Self {
        RedHatBoy {
            state_machine: RedHatBoyStateMachine::Idle(RedHatBoyState::new()),
            sprite_sheet: sheet,
            image,
        }
    }

    fn frame_name(&self) -> String {
        format!(
            "{} ({}).png",
            self.state_machine.frame_name(),
            (self.state_machine.context().frame / 3) + 1
        )
    }

    fn current_sprite(&self) -> Option<&Cell> {
        self.sprite_sheet.frames.get(&self.frame_name())
    }

    fn draw(&self, renderer: &Renderer) {
        let sprite = self.current_sprite().expect("Cell not found");
        renderer.draw_image(
            &self.image,
            &Rect::new_from_x_y(
                sprite.frame.x.into(),
                sprite.frame.y.into(),
                sprite.frame.w.into(),
                sprite.frame.h.into(),
            ),
            &self.destination_box(),
        );
    }

    fn destination_box(&self) -> Rect {
        let sprite = self.current_sprite().expect("Cell not found");

        Rect::new_from_x_y(
            self.state_machine.context().position.x + sprite.sprite_source_size.x,
            self.state_machine.context().position.y + sprite.sprite_source_size.y,
            sprite.frame.w,
            sprite.frame.h,
        )
    }

    fn bounding_box(&self) -> Rect {
        const X_OFFSET: i16 = 18;
        const Y_OFFSET: i16 = 14;
        const W_OFFSET: i16 = 28;
        let mut bounding_box = self.destination_box();
        bounding_box.set_x(bounding_box.x() + X_OFFSET);
        bounding_box.w -= W_OFFSET;
        bounding_box.set_y(bounding_box.y() + Y_OFFSET);
        bounding_box.h -= Y_OFFSET;
        bounding_box
    }

    fn pos_y(&self) -> i16 {
        self.state_machine.context().position.y
    }

    fn velocity_y(&self) -> i16 {
        self.state_machine.context().velocity.y
    }

    fn update(&mut self) {
        self.state_machine = self.state_machine.update();
    }

    fn run_right(&mut self) {
        self.state_machine = self.state_machine.transition(Event::Run);
    }

    fn slide(&mut self) {
        self.state_machine = self.state_machine.transition(Event::Slide);
    }

    fn jump(&mut self) {
        self.state_machine = self.state_machine.transition(Event::Jump);
    }

    fn knock_out(&mut self) {
        self.state_machine = self.state_machine.transition(Event::KnockOut);
    }

    fn land_on(&mut self, position: i16) {
        self.state_machine = self.state_machine.transition(Event::Land(position))
    }

    fn walk_speed(&self) -> i16 {
        self.state_machine.context().velocity.x
    }
}

use red_hat_boy_states::*;

mod red_hat_boy_states {
    use super::HEIGHT;
    use crate::engine::Point;
    // 地面の高さ
    const FLOOR: i16 = 479;
    const PLAYER_HEIGHT: i16 = HEIGHT - FLOOR;
    const STARTING_POINT: i16 = -20;
    // rhb.jsonにおけるフレームの名前
    const IDLE_FRAME_NAME: &str = "Idle";
    const RUN_FRAME_NAME: &str = "Run";
    const SLIDING_FRAME_NAME: &str = "Slide";
    const JUMPING_FRAME_NAME: &str = "Jump";
    const FALLING_FRAME_NAME: &str = "Dead";
    // rhb.jsonにおけるフレームの枚数*3
    const IDLE_FRAMES: u8 = 30;
    const RUNNING_FRAMES: u8 = 24;
    const SLIDING_FRAMES: u8 = 15;
    const JUMPING_FRAMES: u8 = 36;
    const FALLING_FRAMES: u8 = 30;

    const RUNNING_SPEED: i16 = 3;
    const JUMP_SPEED: i16 = -25;
    const GRAVITY: i16 = 1;
    const TERMINAL_VELOCITY: i16 = 20;

    #[derive(Copy, Clone)]
    pub struct RedHatBoyContext {
        pub frame: u8,
        pub position: Point,
        pub velocity: Point,
    }

    impl RedHatBoyContext {
        pub fn update(mut self, frame_count: u8) -> Self {
            if self.velocity.y < TERMINAL_VELOCITY {
                self.velocity.y += GRAVITY;
            }
            self.frame = (self.frame + 1) % frame_count;
            // self.position += self.velocity;
            self.position.y += self.velocity.y;
            self.position.y = self.position.y.min(FLOOR);
            self
        }

        fn reset_frame(mut self) -> Self {
            self.frame = 0;
            self
        }

        fn run_right(mut self) -> Self {
            self.velocity.x += RUNNING_SPEED;
            self
        }

        fn set_vertical_velocity(mut self, y: i16) -> Self {
            self.velocity.y = y;
            self
        }

        fn stop(mut self) -> Self {
            self.velocity.x = 0;
            self.velocity.y = GRAVITY;
            self
        }

        fn set_on(mut self, position: i16) -> Self {
            let position = position - PLAYER_HEIGHT;
            self.position.y = position;
            self
        }
    }

    #[derive(Copy, Clone)]
    pub struct RedHatBoyState<S> {
        pub context: RedHatBoyContext,
        _state: S,
    }
    impl<S> RedHatBoyState<S> {
        pub fn context(&self) -> &RedHatBoyContext {
            &self.context
        }
    }

    #[derive(Copy, Clone)]
    pub struct Idle;
    impl RedHatBoyState<Idle> {
        pub fn new() -> Self {
            RedHatBoyState {
                context: RedHatBoyContext {
                    frame: 0,
                    position: Point {
                        x: STARTING_POINT,
                        y: FLOOR,
                    },
                    velocity: Point { x: 0, y: 0 },
                },
                _state: Idle {},
            }
        }

        pub fn run(self) -> RedHatBoyState<Running> {
            RedHatBoyState {
                context: self.context.reset_frame().run_right(),
                _state: Running {},
            }
        }

        pub fn frame_name(&self) -> &str {
            IDLE_FRAME_NAME
        }

        pub fn update(mut self) -> Self {
            self.context = self.context.update(IDLE_FRAMES);
            self
        }
    }

    #[derive(Copy, Clone)]
    pub struct Running;
    impl RedHatBoyState<Running> {
        pub fn frame_name(&self) -> &str {
            RUN_FRAME_NAME
        }

        pub fn update(mut self) -> Self {
            self.context = self.context.update(RUNNING_FRAMES);
            self
        }

        pub fn slide(self) -> RedHatBoyState<Sliding> {
            RedHatBoyState {
                context: self.context.reset_frame(),
                _state: Sliding {},
            }
        }

        pub fn jump(self) -> RedHatBoyState<Jumping> {
            RedHatBoyState {
                context: self.context.set_vertical_velocity(JUMP_SPEED).reset_frame(),
                _state: Jumping {},
            }
        }

        pub fn knock_out(self) -> RedHatBoyState<Falling> {
            RedHatBoyState {
                context: self.context.reset_frame().stop(),
                _state: Falling {},
            }
        }

        pub fn land_on(self, position: i16) -> RedHatBoyState<Running> {
            RedHatBoyState {
                context: self.context.set_on(position as i16),
                _state: Running {},
            }
        }
    }

    #[derive(Copy, Clone)]
    pub struct Sliding;
    impl RedHatBoyState<Sliding> {
        pub fn frame_name(&self) -> &str {
            SLIDING_FRAME_NAME
        }

        pub fn update(mut self) -> SlidingEndState {
            self.context = self.context.update(SLIDING_FRAMES);
            if self.context.frame + 1 >= SLIDING_FRAMES {
                SlidingEndState::Complete(self.stand())
            } else {
                SlidingEndState::Sliding(self)
            }
        }

        pub fn stand(self) -> RedHatBoyState<Running> {
            RedHatBoyState {
                context: self.context.reset_frame(),
                _state: Running,
            }
        }

        pub fn knock_out(self) -> RedHatBoyState<Falling> {
            RedHatBoyState {
                context: self.context.reset_frame().stop(),
                _state: Falling {},
            }
        }
    }
    pub enum SlidingEndState {
        Complete(RedHatBoyState<Running>),
        Sliding(RedHatBoyState<Sliding>),
    }

    #[derive(Copy, Clone)]
    pub struct Jumping;
    impl RedHatBoyState<Jumping> {
        pub fn frame_name(&self) -> &str {
            JUMPING_FRAME_NAME
        }

        pub fn update(mut self) -> JumpingEndState {
            self.context = self.context.update(JUMPING_FRAMES);
            if self.context.position.y >= FLOOR {
                JumpingEndState::Landing(self.land_on(HEIGHT.into()))
            } else {
                JumpingEndState::Jumping(self)
            }
        }

        pub fn land_on(self, position: i16) -> RedHatBoyState<Running> {
            RedHatBoyState {
                context: self.context.reset_frame().set_on(position as i16),
                _state: Running {},
            }
        }

        pub fn knock_out(self) -> RedHatBoyState<Falling> {
            RedHatBoyState {
                context: self.context.reset_frame().stop(),
                _state: Falling {},
            }
        }
    }
    pub enum JumpingEndState {
        Landing(RedHatBoyState<Running>),
        Jumping(RedHatBoyState<Jumping>),
    }

    #[derive(Copy, Clone)]
    pub struct Falling;

    impl RedHatBoyState<Falling> {
        pub fn frame_name(&self) -> &str {
            FALLING_FRAME_NAME
        }

        pub fn update(mut self) -> FallingEndState {
            self.context = self.context.update(FALLING_FRAMES);
            if self.context.frame + 1 >= FALLING_FRAMES {
                FallingEndState::Complete(self.knock_out())
            } else {
                FallingEndState::Falling(self)
            }
        }

        pub fn knock_out(mut self) -> RedHatBoyState<KnockedOut> {
            RedHatBoyState {
                context: self.context,
                _state: KnockedOut {},
            }
        }
    }

    #[derive(Copy, Clone)]
    pub struct KnockedOut;
    impl RedHatBoyState<KnockedOut> {
        pub fn frame_name(&self) -> &str {
            FALLING_FRAME_NAME
        }
    }

    pub enum FallingEndState {
        Complete(RedHatBoyState<KnockedOut>),
        Falling(RedHatBoyState<Falling>),
    }
}

#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    browser::spawn_local(async move {
        let game = WalkTheDog::new();

        GameLoop::start(game)
            .await
            .expect("Could not start game loop");
    });

    Ok(())
}
