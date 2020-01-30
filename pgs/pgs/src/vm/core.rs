use super::{
    is::{
        Opcode
    },
    address::{
        Address,
        AddressType
    },
    register::{
        Register,
        RegisterAccess
    }
};
use crate::{
    codegen::{
        program::Program
    },
    api::{
        module::Module,
        function::*
    }
};

use std::{
    collections::{
        VecDeque,
        HashMap
    },
    mem::{
        size_of,
        size_of_val
    },
    cell::{
        RefCell
    },
    ops::Range,
    fmt::{
        Debug,
        Display,
        Formatter,
        Result as FmtResult
    },
    error::Error
};

use serde::{
    de::{
        DeserializeOwned
    },
    Serialize
};

use bincode::{
    serialize,
    deserialize
};

use rand::{
    Rng,
    RngCore,
    thread_rng
};

pub type CoreResult<T> = Result<T, CoreError>;

pub const STACK_GROW_INCREMENT: usize = 1024;
pub const STACK_GROW_THRESHOLD: usize = 64;
pub const SWAP_SPACE_SIZE: usize = 64;

pub struct Core {
    stack: Vec<u8>,
    heap: Vec<u8>,
    heap_pointers: Vec<Range<usize>>,
    foreign_functions: HashMap<u64, Box<dyn FnMut(&mut Core) -> FunctionResult<()>>>,
    swap: Vec<u8>,
    program: Option<Program>,
    stack_frames: VecDeque<usize>,
    call_stack: VecDeque<usize>,
    registers: [Register; 16],
    ip: Register,
    sp: Register,
}

#[derive(Debug)]
pub enum CoreError {
    Unknown,
    NoProgram,
    UnimplementedOpcode(Opcode),
    OperatorDeserialize,
    OperatorSerialize,
    EmptyCallStack,
    UnknownFunctionUid,
    InvalidStackPointer,
    InvalidRegister,
    NoReturnValue,
    Halted(u8)
}

impl Display for CoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{:?}", self)
    }
}

impl Error for CoreError {
}

impl Core {
    pub fn new(stack_size: usize) -> Core {
        let mut stack = Vec::new();
        stack.resize(stack_size, 0);
        let mut swap = Vec::new();
        swap.resize(SWAP_SPACE_SIZE, 0);
        let mut sp = Register::new();
        let address = Address::new(0, AddressType::Stack);
        sp.set::<u64>(address.into());
        Core {
            program: None,
            swap: swap,
            stack: stack,
            heap: Vec::new(),
            heap_pointers: Vec::new(),
            foreign_functions: HashMap::new(),
            stack_frames: VecDeque::new(),
            call_stack: VecDeque::new(),
            registers: [Register::new(); 16],
            ip: Register::new(),
            sp: sp
        }
    }

    #[inline]
    pub fn load_program(&mut self, program: Program) {
        self.program = Some(program);
    }

    #[inline]
    pub fn program_len(&self) -> CoreResult<usize> {
        let program = self.program.as_ref()
            .ok_or(CoreError::Unknown)?;
        Ok(
            program.code.len()
        )
    }

    #[inline]
    pub fn get_stack_size(&self) -> usize {
        let sp_raw: u64 = self.sp.get();
        let sp_addr = Address::from(sp_raw);
        sp_addr.real_address as usize
    }

    #[inline]
    pub fn get_opcode(&mut self) -> CoreResult<Opcode> {
        let program = self.program.as_ref()
            .ok_or(CoreError::NoProgram)?;
        //println!("Getting opcode {:X} ...", program.code[self.ip]);
        //println!("Opcode: {:?}", Opcode::from(program.code[self.ip])),
        let ip: usize = self.ip.get();
        let opcode = Opcode::from(program.code[ip]);
        self.ip.inc(1usize);
        
        Ok(
            opcode
        )
    }

    #[inline]
    pub fn run(&mut self) -> CoreResult<()> {
        self.run_at(0)
    }
    
    #[inline]
    pub fn run_fn(&mut self, uid: u64) -> CoreResult<()> {
        let fn_offset = {
            let program = self.program.as_ref()
                .ok_or(CoreError::NoProgram)?;
            program.functions.get(&uid)
                .ok_or(CoreError::NoProgram)?
                .clone()
        };

        self.run_at(fn_offset)
    }

    pub fn run_at(&mut self, offset: usize) -> CoreResult<()> {
        self.ip.set(offset);
        let program_len = self.program_len()?;
        //println!("Program length: {}", program_len);
        while self.ip.get::<usize>() < program_len {
            //println!("ip: {}", self.ip);
            let opcode = self.get_opcode()?;
            //println!("Stack values: {:?}", &self.stack[0..self.sp]);
            //println!("IP: {}", self.ip);

            match opcode {
                Opcode::NOOP => {},
                Opcode::HALT => {
                    let err_code: u8 = self.get_op()?;
                    match err_code {
                        1 => {
                            return Err(CoreError::NoReturnValue);
                        },
                        _ => {
                            return Err(CoreError::Halted(err_code))
                        }
                    };
                },
                Opcode::MOVB => {
                    let lhs: u8 = self.get_op()?;
                    let rhs: u8 = self.get_op()?;
                    let boolean: bool = {
                        self.reg(lhs)?.get()
                    };
                    self.reg(rhs)?.set(boolean);
                },
                Opcode::MOVF => {
                    let lhs: u8 = self.get_op()?;
                    let rhs: u8 = self.get_op()?;
                    let float: f32 = {
                        self.reg(lhs)?.get()
                    };
                    self.reg(rhs)?.set(float);
                },
                Opcode::MOVI => {
                    let lhs: u8 = self.get_op()?;
                    let rhs: u8 = self.get_op()?;
                    let int64: i64 = {
                        self.reg(lhs)?.get()
                    };
                    self.reg(rhs)?.set(int64);
                },
                Opcode::MOVA => {
                    let lhs: u8 = self.get_op()?;
                    let rhs: u8 = self.get_op()?;
                    let uint64: u64 = {
                        self.reg(lhs)?.get()
                    };
                    self.reg(rhs)?.set(uint64);
                },
                Opcode::MOVB_A => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.mem_mov_n((lhs_addr, lhs_offset), (rhs_addr, rhs_offset), 1)?;
                },
                Opcode::MOVF_A => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.mem_mov_n((lhs_addr, lhs_offset), (rhs_addr, rhs_offset), 4)?;
                },
                Opcode::MOVI_A => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.mem_mov_n((lhs_addr, lhs_offset), (rhs_addr, rhs_offset), 8)?;
                },
                Opcode::MOVA_A => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.mem_mov_n((lhs_addr, lhs_offset), (rhs_addr, rhs_offset), 8)?;
                },
                Opcode::MOVN_A => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let n: usize = self.get_op::<u32>()? as usize;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.mem_mov_n((lhs_addr, lhs_offset), (rhs_addr, rhs_offset), n)?;
                },
                Opcode::MOVB_AR => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let boolean: bool = self.mem_get((lhs_addr, lhs_offset))?;
                    self.reg(rhs_reg)?.set(boolean);
                },
                Opcode::MOVF_AR => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let float: f32 = self.mem_get((lhs_addr, lhs_offset))?;
                    self.reg(rhs_reg)?.set(float)
                },
                Opcode::MOVI_AR => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let int64: i64 = self.mem_get((lhs_addr, lhs_offset))?;
                    self.reg(rhs_reg)?.set(int64)
                },
                Opcode::MOVA_AR => {
                    let lhs_reg: u8 = self.get_op()?;
                    let lhs_offset: i16 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let lhs_addr: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let uint64: u64 = self.mem_get((lhs_addr, lhs_offset))?;
                    self.reg(rhs_reg)?.set(uint64)
                },
                Opcode::MOVB_RA => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let boolean: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.mem_set((rhs_addr, rhs_offset), boolean)?;
                },
                Opcode::MOVF_RA => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let float: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.mem_set((rhs_addr, rhs_offset), float)?;
                },
                Opcode::MOVI_RA => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let int64: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.mem_set((rhs_addr, rhs_offset), int64)?;
                },
                Opcode::MOVA_RA => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let rhs_offset: i16 = self.get_op()?;
                    let rhs_addr: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let uint64: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.mem_set((rhs_addr, rhs_offset), uint64)?;
                },
                Opcode::LDB => {
                    let boolean: bool = self.get_op()?;
                    let lhs_reg: u8 = self.get_op()?;
                    self.reg(lhs_reg)?.set(boolean);
                },
                Opcode::LDF => {
                    let float: f32 = self.get_op()?;
                    let lhs_reg: u8 = self.get_op()?;
                    self.reg(lhs_reg)?.set(float);
                },
                Opcode::LDI => {
                    let int64: i64 = self.get_op()?;
                    let lhs_reg: u8 = self.get_op()?;
                    self.reg(lhs_reg)?.set(int64);
                },
                Opcode::LDA => {
                    let uint64: u64 = self.get_op()?;
                    let lhs_reg: u8 = self.get_op()?;
                    self.reg(lhs_reg)?.set(uint64)
                },
                Opcode::ADDI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs);
                },
                Opcode::MULI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs);
                },
                Opcode::DIVI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs)
                },
                Opcode::ADDI_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: i64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBI_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: i64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs);
                },
                Opcode::MULI_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: i64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs);
                },
                Opcode::DIVI_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: i64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs);
                },
                Opcode::ADDU => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBU => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs)
                },
                Opcode::MULU => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs)
                },
                Opcode::DIVU => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs)
                },
                Opcode::ADDU_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: u64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBU_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: u64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs);
                },
                Opcode::MULU_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: u64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs);
                },
                Opcode::DIVU_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: u64 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs);
                },
                Opcode::ADDF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs);
                },
                Opcode::MULF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs);
                },
                Opcode::DIVF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs);
                },
                Opcode::ADDF_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: f32 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs + rhs);
                },
                Opcode::SUBF_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: f32 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs - rhs);
                },
                Opcode::MULF_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: f32 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs * rhs);
                },
                Opcode::DIVF_I => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs: f32 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs / rhs);
                },
                Opcode::JMP => {
                    let target_ip: u64 = self.get_op()?;
                    self.ip.set(target_ip);
                },
                Opcode::JMPT => {
                    let lhs_reg: u8 = self.get_op()?;
                    let target_ip: u64 = self.get_op()?;
                    let lhs: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    if lhs {
                        self.ip.set(target_ip);
                    }
                },
                Opcode::JMPF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let target_ip: u64 = self.get_op()?;
                    let lhs: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    if !lhs {
                        self.ip.set(target_ip);
                    }
                },
                Opcode::DJMP => {
                    let lhs_reg: u8 = self.get_op()?;
                    let target_ip: u64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.ip.set(target_ip);
                },
                Opcode::DJMPT => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_ip: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let lhs: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    if lhs {
                        self.ip.set(target_ip);
                    }
                },
                Opcode::DJMPF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_ip: u64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    let lhs: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    if !lhs {
                        self.ip.set(target_ip);
                    }
                },
                Opcode::CALL => {
                    self.call()?;
                },
                Opcode::RET => {
                    // Special case if function was called externally, the callstack is empty
                    if self.call_stack.len() == 0 {
                        break;
                    }
                    self.ret()?;
                },
                Opcode::NOT => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let lhs: bool = {
                        self.reg(lhs_reg)?.get()
                    };
                    self.reg(rhs_reg)?.set(!lhs);
                },
                Opcode::EQI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs == rhs);
                },
                Opcode::NEQI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs != rhs);
                },
                Opcode::LTI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs < rhs);
                },
                Opcode::GTI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs > rhs);
                },
                Opcode::LTEQI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs <= rhs);
                },
                Opcode::GTEQI => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: i64 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: i64 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs >= rhs);
                },
                Opcode::EQF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs == rhs);
                },
                Opcode::NEQF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs != rhs);
                },
                Opcode::LTF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs < rhs);
                },
                Opcode::GTF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs > rhs);
                },
                Opcode::LTEQF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs <= rhs);
                },
                Opcode::GTEQF => {
                    let lhs_reg: u8 = self.get_op()?;
                    let rhs_reg: u8 = self.get_op()?;
                    let target_reg: u8 = self.get_op()?;
                    let lhs: f32 = {
                        self.reg(lhs_reg)?.get()
                    };
                    let rhs: f32 = {
                        self.reg(rhs_reg)?.get()
                    };
                    self.reg(target_reg)?.set(lhs >= rhs);
                },
                _ => {
                    return Err(CoreError::UnimplementedOpcode(opcode));
                }
            };
        }
        Ok(())
    }

    fn mem_mov_n(&mut self, lhs: (u64, i16), rhs: (u64, i16), n: usize) -> CoreResult<()> {
        let lhs_addr = Address::from(lhs.0).with_offset(lhs.1);
        let rhs_addr = Address::from(rhs.0).with_offset(rhs.1);

        let source_addr = lhs_addr.real_address as usize;
        let target_addr = rhs_addr.real_address as usize;

        let bytes = {
            let source: &[u8] = match lhs_addr.address_type {
                AddressType::Stack => {
                    &self.stack
                },
                AddressType::Program => {
                    let program = self.program.as_ref()
                        .ok_or(CoreError::Unknown)?;
                    &program.code
                },
                AddressType::Swap => {
                    &self.swap
                },
                _ => return Err(CoreError::Unknown)
            };
            
            let mut ret = Vec::with_capacity(n);
            ret.resize(n, 0);

            for i in 0..n {
                ret[i] = source[source_addr + i];
            }

            ret
        };

        match rhs_addr.address_type {
            AddressType::Stack => {
                for i in 0..n {
                    self.stack[target_addr + i] = bytes[i];
                }
            },
            AddressType::Program => {
                let program = self.program.as_mut()
                    .ok_or(CoreError::Unknown)?;
                for i in 0..n {
                    program.code[target_addr + i] = bytes[i];
                }
            },
            AddressType::Swap => {
                for i in 0..n {
                    self.swap[target_addr + i] = bytes[i];
                }
            },
            _ => return Err(CoreError::Unknown)
        };

        Ok(())
    }

    fn mem_get_n(&self, addr: (u64, i16), n: usize) -> CoreResult<Vec<u8>> {
        let mut data = Vec::with_capacity(n);
        data.resize(n, 0);

        let lhs_addr = Address::from(addr.0).with_offset(addr.1);

        let source_addr = lhs_addr.real_address as usize;

        let source: &[u8] = match lhs_addr.address_type {
            AddressType::Stack => {
                &self.stack
            },
            AddressType::Program => {
                let program = self.program.as_ref()
                    .ok_or(CoreError::Unknown)?;
                &program.code
            },
            AddressType::Swap => {
                &self.swap
            },
            _ => return Err(CoreError::Unknown)
        };

        for i in 0..n {
            data[i] = source[source_addr + i];
        }

        Ok(data)
    }
    
    #[inline]
    pub fn mem_get_string(&self, addr: u64) -> CoreResult<String> {
        let string_size: u64 = self.mem_get((addr, 0))?;
        let string_addr: u64 = self.mem_get((addr + 8, 0))?;
        let string_data = self.mem_get_n((string_addr, 0), string_size as usize)?;
        String::from_utf8(string_data)
            .map_err(|_| CoreError::OperatorDeserialize)
    }

    #[inline]
    pub fn mem_get<T: DeserializeOwned>(&self, addr: (u64, i16)) -> CoreResult<T> {
        let n = size_of::<T>();

        let data = self.mem_get_n(addr, n)?;

        deserialize(&data)
            .map_err(|_| CoreError::OperatorDeserialize)
    }
    
    #[inline]
    pub fn mem_set<T: Serialize>(&mut self, addr: (u64, i16), item: T) -> CoreResult<()> {
        let n = size_of::<T>();

        let lhs_addr = Address::from(addr.0).with_offset(addr.1);

        let data = serialize(&item)
            .map_err(|_| CoreError::OperatorSerialize)?;

        let target_addr = lhs_addr.real_address as usize;
        
        match lhs_addr.address_type {
            AddressType::Stack => {
                for i in 0..n {
                    self.stack[target_addr + i] = data[i];
                }
            },
            AddressType::Program => {
                let program = self.program.as_mut()
                    .ok_or(CoreError::Unknown)?;
                for i in 0..n {
                    program.code[target_addr + i] = data[i];
                }
            },
            _ => return Err(CoreError::Unknown)
        };

        Ok(())
    }

    #[inline]
    pub fn reg(&mut self, reg: u8) -> CoreResult<&mut Register> {
        if reg == 16 {
            return Ok(&mut self.sp);
        }
        if reg == 17 {
            return Ok(&mut self.ip);
        }
        else if reg < 16 {
            return Ok(&mut self.registers[reg as usize]);
        }
        else {
            return Err(CoreError::InvalidRegister);
        }
    }

    #[inline]
    fn call(&mut self) -> CoreResult<()> {
        let fn_uid: u64 = self.get_op()?;
        if let Some(mut closure) = self.foreign_functions.remove(&fn_uid) {
            //println!("Executing foreign function...");
            closure(self)
                .map_err(|_| CoreError::Unknown)?;
            self.foreign_functions.insert(fn_uid, closure);
            return Ok(());
        }

        let program = self.program.as_ref()
            .ok_or(CoreError::NoProgram)?;

        let new_ip = program.functions.get(&fn_uid)
            .ok_or(CoreError::UnknownFunctionUid)?;
        
        let old_ip: usize = self.ip.get();
        self.call_stack.push_front(old_ip);
        self.ip.set(*new_ip);

        Ok(())
    }

    #[inline]
    fn ret(&mut self) -> CoreResult<()> {
        let old_ip = self.call_stack.pop_front()
            .ok_or(CoreError::EmptyCallStack)?;
        self.ip.uint64 = old_ip as u64;
        Ok(())
    }

    #[inline]
    fn get_op<T: DeserializeOwned>(&mut self) -> CoreResult<T> {
        let op_size = size_of::<T>();

        let program = &self.program.as_ref().unwrap().code;

        let tmp_ip = self.ip.get::<usize>();

        let raw_bytes: &[u8] = &program[tmp_ip..tmp_ip + op_size];
        //println!("get_op raw bytes: {:?}", raw_bytes);

        let ret: T = deserialize(raw_bytes)
            .map_err(|_| CoreError::OperatorDeserialize)?;

        self.ip.inc(op_size);

        Ok(ret)
    }

    #[inline]
    pub fn push_stack<T: Serialize>(&mut self, item: T) -> CoreResult<()> {
        let op_size = size_of::<T>();

        let raw_bytes = serialize(&item)
            .map_err(|_| CoreError::OperatorSerialize)?;

        let tmp_sp = self.sp.get::<usize>();

        if self.stack.len() - (tmp_sp + op_size) <= STACK_GROW_THRESHOLD {
            self.stack.resize(self.stack.len() + STACK_GROW_INCREMENT, 0);
        } 
        
        for i in 0..op_size {
            self.stack[tmp_sp + i] = raw_bytes[i];
        }
        
        self.sp.inc(op_size);

        Ok(())
    }

    #[inline]
    pub fn pop_stack<T: DeserializeOwned>(&mut self) -> CoreResult<T> {
        let op_size = size_of::<T>();

        let mut raw_bytes = Vec::with_capacity(op_size);
        raw_bytes.resize(op_size, 0);

        let sp_raw = self.sp.get::<u64>();
        let sp_addr = Address::from(sp_raw);

        if op_size > self.sp.get::<usize>() {
            return Err(CoreError::InvalidStackPointer);
        }

        let mut source_addr = sp_addr.real_address as usize;
        source_addr -= op_size;

        for i in 0..op_size {
            raw_bytes[i] = self.stack[source_addr + i];
        }

        self.sp.dec(op_size);

        deserialize(&raw_bytes)
            .map_err(|_| CoreError::Unknown)
    }

    #[inline]
    fn save_swap<T: Serialize>(&mut self, item: T) -> CoreResult<()> {
        let op_size = size_of::<T>();

        if self.swap.len() < op_size {
            self.swap.resize(self.swap.len() + op_size, 0);
        }

        let raw_bytes = serialize(&item)
            .map_err(|_| CoreError::OperatorSerialize)?;

        for i in 0..op_size {
            self.swap[i] = raw_bytes[i];
        }

        Ok(())
    }

    pub fn register_foreign_module(&mut self, module: Module) -> CoreResult<()> {
        for function in module.functions {
            let raw_callback = function.raw_callback
                .ok_or(CoreError::Unknown)?;
            let uid = function.uid
                .ok_or(CoreError::UnknownFunctionUid)?;
            self.foreign_functions.insert(uid, raw_callback);
        }
        for (_, module) in module.modules {
            self.register_foreign_module(module)?;
        }
        Ok(())
    }
}
