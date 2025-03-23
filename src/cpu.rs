
/*
命令がロードされた後のcpuのメモリ内の一例
_______________________
アドレス|命令orオペランド

0x8000|a9
0x8001|c0
0x8002|aa
0x8003|e8
0x8004|00
*/

use crate::opcodes;
use std::collections::HashMap;

/*
スタックは0x0100-0x01FF
*/
const STACK:u16=0x0100;
const STACK_RESET:u8=0xfd;

pub struct CPU{
    pub register_a:u8,//u8<=2^8-1=255
    pub register_x:u8,
    pub register_y:u8,
    pub status: u8,//NV BDIZC
    ///  7 6 5 4 3 2 1 0
    ///  N V _ B D I Z C
    ///  | |   | | | | +--- Carry Flag      //符号なし演算での桁あふれ
    ///  | |   | | | +----- Zero Flag
    ///  | |   | | +------- Interrupt Disable
    ///  | |   | +--------- Decimal Mode (not used on NES)
    ///  | |   +----------- Break Command
    ///  | +--------------- Overflow Flag   //符号あり演算で127超え等
    ///  +----------------- Negative Flag
    ///
    pub program_counter:u16,
    pub stack_pointer:u8,
    memory:[u8;0xFFFF],
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub struct CpuFlags;
impl CpuFlags{
    const CARRY             :u8=0b0000_0001;
    const ZERO              :u8=0b0000_0010;
    const INTERRUPT_DISABLE :u8=0b0000_0100;
    const DECIMAL_MODE      :u8=0b0000_1000;
    const BREAK             :u8=0b0001_0000;
    const BREAK2            :u8=0b0010_0000;
    const OVERFLOW          :u8=0b0100_0000;
    const NEGATIVE          :u8=0b1000_0000;
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum AddressingMode{
    Immediate,
    ZeroPage,
    ZeroPage_X,
    ZeroPage_Y,
    Absolute,
    Absolute_X,
    Absolute_Y,
    Indirect_X,
    Indirect_Y,
    NoneAddressing,
}

trait Mem{
    fn mem_read(&self,addr:u16)->u8;
    fn mem_write(&mut self, addr:u16, data:u8);

    fn mem_read_u16(&self,pos: u16)->u16{
        let lo=self.mem_read(pos) as u16;
        let hi=self.mem_read(pos+1) as u16;
        (hi<<8)|lo
    }
    fn mem_write_u16(&mut self,pos: u16,data: u16){
        let hi=(data>>8) as u8;
        let lo=(data&0xff) as u8;
        self.mem_write(pos,lo);
        self.mem_write(pos+1,hi);
    }
}

pub trait UpdateFlag{
    fn set_flag(&mut self,flag:u8);
    fn clear_flag(&mut self,flag:u8);
    fn contain_flag(&self,flag:u8)->bool;
}

impl UpdateFlag for CPU{
    fn set_flag(&mut self,flag:u8){
        self.status|=flag;
    }
    fn clear_flag(&mut self,flag:u8){
        self.status&=!flag;
    }
    fn contain_flag(&self,flag:u8)->bool{
        self.status&flag!=0
    }
}

impl Mem for CPU{
    fn mem_read(&self,addr:u16)->u8{
        self.memory[addr as usize]
    }

    fn mem_write(&mut self,addr:u16,data:u8){
        self.memory[addr as usize]=data;
    }
}

impl CPU{
    pub fn new()->Self{
        CPU{
            register_a:0,
            register_x:0,
            register_y:0,
            program_counter:0,
            stack_pointer:STACK_RESET,
            status:0,
            memory:[0;0xFFFF]
        }
    }

    fn get_operand_address(&self,mode:&AddressingMode)->u16{
        match mode{
            AddressingMode::Immediate=>self.program_counter,
            AddressingMode::ZeroPage=>self.mem_read(self.program_counter) as u16,
            AddressingMode::Absolute=>self.mem_read_u16(self.program_counter),
            AddressingMode::ZeroPage_X=>{//
                let pos=self.mem_read(self.program_counter);
                let addr=pos.wrapping_add(self.register_x) as u16;//溢れた桁を無視する加算
                addr
            }
            AddressingMode::ZeroPage_Y=>{
                let pos=self.mem_read(self.program_counter);
                let addr=pos.wrapping_add(self.register_y) as u16;//溢れた桁を無視する加算
                addr
            }
            AddressingMode::Absolute_X=>{
                let base=self.mem_read_u16(self.program_counter);
                let addr=base.wrapping_add(self.register_x as u16);
                addr
            }
            AddressingMode::Absolute_Y=>{
                let base=self.mem_read_u16(self.program_counter);
                let addr=base.wrapping_add(self.register_y as u16);
                addr
            }
            AddressingMode::Indirect_X=>{
                let base=self.mem_read(self.program_counter);

                let pos: u8=(base as u8).wrapping_add(self.register_x);
                let lo=self.mem_read(pos as u16);
                let hi=self.mem_read(pos.wrapping_add(1) as u16);
                (hi as u16)<<8|(lo as u16)
            }
            AddressingMode::Indirect_Y=>{
                let base=self.mem_read(self.program_counter);

                let lo=self.mem_read(base as u16);
                let hi=self.mem_read(base.wrapping_add(1) as u16);
                let deref_base=(hi as u16)<<8|(lo as u16);
                let deref=deref_base.wrapping_add(self.register_y as u16);
                deref
            }
            AddressingMode::NoneAddressing=>panic!("mode {:?} is not supported",mode)
        }
    }

    fn update_zero_flag(&mut self,result:u8){
        if result==0{
            self.status|=0b0000_0010;
        }else{
            self.status&=0b1111_1101;
        }
    }

    fn update_negative_flag(&mut self,result:u8){
        if result&0b1000_0000!=0{
            self.status|=0b1000_0000;
        }else{
            self.status&=0b0111_1111;
        }
    }
    fn update_zero_and_negative_flags(&mut self,result:u8){
        self.update_zero_flag(result);
        self.update_negative_flag(result)
    }

    fn set_register_a(&mut self,value:u8){
        self.register_a=value;
        self.update_zero_and_negative_flags(self.register_a);
    }

    fn lda(&mut self, mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let value=self.mem_read(addr);

        self.set_register_a(value);
    }

    fn ldx(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        self.register_x=data;
        self.update_zero_and_negative_flags(self.register_x);
    }

    fn ldy(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        self.register_y=data;
        self.update_zero_and_negative_flags(self.register_y);
    }

    fn sta(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        self.mem_write(addr,self.register_a);
    }

    fn and(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        self.set_register_a(data & self.register_a);
    }
    
    //exclusive OR /exclusive...排他的な
    //XOR
    fn eor(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        self.set_register_a(data ^ self.register_a);
    }

    fn ora(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        self.set_register_a(data|self.register_a);
    }

    fn tax(&mut self){
        self.register_x=self.register_a;
        self.update_zero_and_negative_flags(self.register_x);
    }

    fn inx(&mut self){
        self.register_x=self.register_x.wrapping_add(1);
        self.update_zero_and_negative_flags(self.register_x);
    }

    fn iny(&mut self){
        self.register_y=self.register_y.wrapping_add(1);
        self.update_zero_and_negative_flags(self.register_y);
    }

    fn add_to_register_a(&mut self,data:u8){
        let sum=self.register_a as u16
                +data as u16
                +(if self.status|0b1111_1110==0b1111_1111 {1}else{0}) as u16;
        let carry_flag=sum>0xff;
        if carry_flag{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }

        let result=sum as u8;

        //over flow flag
        /*
        例　0b0111_1111 + 0b0000_0010 = 0b1000_0001
            register_a(正)   data(正)     result(負)
        */
        if (data^result)&(result^self.register_a)&0b1000_0000!=0{
            self.set_flag(CpuFlags::OVERFLOW);
        }else{
            self.clear_flag(CpuFlags::OVERFLOW);
        }

        self.set_register_a(result);
    }

    fn sbc(&mut self,mode:&AddressingMode){//substract with carry
        let addr=self.get_operand_address(&mode);
        let data=self.mem_read(addr);
        self.add_to_register_a((data as i8).wrapping_neg().wrapping_sub(1) as u8);
    }

    fn adc(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let value=self.mem_read(addr);
        self.add_to_register_a(value);
    }


    fn stack_pop(&mut self)->u8{
        self.stack_pointer=self.stack_pointer.wrapping_add(1);
        self.mem_read(STACK+self.stack_pointer as u16)
    }

    fn stack_push(&mut self,data:u8){
        self.mem_write(STACK+self.stack_pointer as u16,data);
        self.stack_pointer=self.stack_pointer.wrapping_sub(1);
    }

    fn stack_pop_u16(&mut self)->u16{
        let lo=self.stack_pop() as u16;
        let hi=self.stack_pop() as u16;
        (hi<<8)|lo
    }

    fn stack_push_u16(&mut self, data:u16){
        let hi=(data>>8) as u8;
        let lo=(data&0x00ff) as u8;
        self.stack_push(hi);
        self.stack_push(lo);
    }

    fn asl_accumulator(&mut self) {
        let mut data = self.register_a;
        if data >> 7 == 1 {
            self.set_flag(CpuFlags::CARRY);
        } else {
            self.clear_flag(CpuFlags::CARRY);
        }
        data = data << 1;
        self.set_register_a(data)
    }

    fn asl(&mut self, mode: &AddressingMode) -> u8 {//arithmetic shift left
        let addr = self.get_operand_address(mode);
        let mut data = self.mem_read(addr);
        if data >> 7 == 1 {
            self.set_flag(CpuFlags::CARRY);
        } else {
            self.clear_flag(CpuFlags::CARRY);
        }
        data = data << 1;
        self.mem_write(addr, data);
        self.update_zero_and_negative_flags(data);
        data
    }

    fn lsr_accumulator(&mut self) {
        let mut data = self.register_a;
        if data & 1 == 1 {
            self.set_flag(CpuFlags::CARRY);
        } else {
            self.clear_flag(CpuFlags::CARRY);
        }
        data = data >> 1;
        self.set_register_a(data)
    }

    fn lsr(&mut self,mode:&AddressingMode)->u8{
        let addr=self.get_operand_address(mode);
        let mut data=self.mem_read(addr);
        if data&1==1{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }
        data=data>>1;
        self.mem_write(addr,data);
        self.update_zero_and_negative_flags(data);
        data
    }

    fn rol_accumulator(&mut self){
        let mut data=self.register_a;
        let old_carry=self.status&0b0000_0001==0b0000_0001;

        if data>>7==1{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }
        data=data<<1;
        if old_carry {
            data=data|1;
        }
        self.set_register_a(data);
    }

    fn rol(&mut self,mode:&AddressingMode)->u8{//rotate left
        let addr=self.get_operand_address(mode);
        let mut data=self.mem_read(addr);
        let old_carry=self.status&0b0000_0001==0b0000_0001;

        if data>>7==1{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }
        data=data<<1;
        if old_carry {
            data=data|1;
        }
        self.mem_write(addr,data);
        self.update_zero_and_negative_flags(data);
        data
    }
    
    fn ror_accumulator(&mut self){
        let mut data=self.register_a;
        let old_carry=self.status&0b0000_0001==0b0000_0001;

        if data&1==1{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }
        data=data>>1;
        if old_carry {
            data=data|0b1000_0000;
        }
        self.set_register_a(data);
    }

    fn ror(&mut self,mode:&AddressingMode)->u8{//rotate left
        let addr=self.get_operand_address(mode);
        let mut data=self.mem_read(addr);
        let old_carry=self.status&0b0000_0001==0b0000_0001;

        if data&1==1{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }
        data=data>>1;
        if old_carry {
            data=data|0b1000_0000;
        }
        self.mem_write(addr,data);
        self.update_zero_and_negative_flags(data);
        data
    }

    fn inc(&mut self,mode:&AddressingMode)->u8{
        let addr=self.get_operand_address(mode);
        let mut data=self.mem_read(addr);
        data=data.wrapping_add(1);
        self.mem_write(addr,data);
        self.update_zero_and_negative_flags(data);
        data
    }

    fn dex(&mut self){
        self.register_x=self.register_x.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.register_x);
    }

    fn dey(&mut self){
        self.register_y=self.register_y.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.register_y);
    }

    fn dec(&mut self,mode:&AddressingMode)->u8{
        let addr=self.get_operand_address(mode);
        let mut data=self.mem_read(addr);
        data=data.wrapping_sub(1);
        self.mem_write(addr,data);
        self.update_zero_and_negative_flags(data);
        data
    }

    fn pla(&mut self){//pull
        let data=self.stack_pop();
        self.set_register_a(data);
    }

    fn plp(&mut self){//pull processor status
        self.status=self.stack_pop();
        self.clear_flag(CpuFlags::BREAK);
        self.set_flag(CpuFlags::BREAK);
    }

    fn php(&mut self){//push processor status
        let tmp=self.status.clone();
        self.clear_flag(CpuFlags::BREAK);
        self.set_flag(CpuFlags::BREAK2);
        self.stack_push(self.status);
        self.status=tmp;
    }

    fn bit(&mut self,mode:&AddressingMode){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        let and=self.register_a&data;
        if and==0{
            self.set_flag(CpuFlags::ZERO);
        }else{
            self.clear_flag(CpuFlags::ZERO);
        }

        if data&0b1000_0000>0 {self.set_flag(CpuFlags::NEGATIVE);}
        if data&0b0100_0000>0 {self.set_flag(CpuFlags::OVERFLOW);}
    }

    fn compare(&mut self,mode:&AddressingMode,compare_with:u8){
        let addr=self.get_operand_address(mode);
        let data=self.mem_read(addr);
        if data<=compare_with{
            self.set_flag(CpuFlags::CARRY);
        }else{
            self.clear_flag(CpuFlags::CARRY);
        }

        self.update_zero_and_negative_flags(compare_with.wrapping_sub(data));
    }

    fn branch(&mut self,condition:bool){
        if condition {
            let jump:i8=self.mem_read(self.program_counter) as i8;
            let jump_addr=self.program_counter.wrapping_add(1).wrapping_add(jump as u16);

            self.program_counter=jump_addr;
        }
    }

    pub fn load_and_run(&mut self,program:Vec<u8>){
        self.load(program);
        self.program_counter=self.mem_read_u16(0xFFFC);
        self.run();
    }

    pub fn reset(&mut self){
        self.register_a=0;
        self.register_x=0;
        self.register_y=0;
        self.stack_pointer=STACK_RESET;
        self.status=0;

        self.program_counter=self.mem_read_u16(0xFFFC);
    }

    pub fn load(&mut self, program:Vec<u8>){
        self.memory[0x8000..(0x8000+program.len())].copy_from_slice(&program[..]);
        self.mem_write_u16(0xFFFC,0x8000);
        /*
        0xFFFCに格納された 2 バイトの値でprogram_counter初期化する必要があります
        */
    }

    pub fn run(&mut self){
        let ref opcodes:HashMap<u8,&'static opcodes::OpCode>=*opcodes::OPCODES_MAP;
        loop{
            let code=self.mem_read(self.program_counter);
            self.program_counter+=1;
            let program_counter_state=self.program_counter;

            let opcode=opcodes.get(&code).expect(&format!("opcode {:x} is not recognized",code));

            match code{
                0xa9|0xa5|0xb5|0xad|0xbd|0xb9|0xa1|0xb1=>{
                    self.lda(&opcode.mode);
                }
                0xaa => self.tax(),
                0xe8 => self.inx(),
                0x00 => return,

                /* CLD */ 0xd8 => self.clear_flag(CpuFlags::DECIMAL_MODE),

                /* CLI */ 0x58 => self.clear_flag(CpuFlags::INTERRUPT_DISABLE),

                /* CLV */ 0xb8 => self.clear_flag(CpuFlags::OVERFLOW),

                /* CLC */ 0x18 => self.clear_flag(CpuFlags::CARRY),

                /* SEC */ 0x38 => self.set_flag(CpuFlags::CARRY),

                /* SEI */ 0x78 => self.set_flag(CpuFlags::INTERRUPT_DISABLE),

                /* SED */ 0xf8 => self.set_flag(CpuFlags::DECIMAL_MODE),

                /* PHA */ 0x48 => self.stack_push(self.register_a),

                /* PLA */
                0x68 => {
                    self.pla();
                }

                /* PHP */
                0x08 => {
                    self.php();
                }

                /* PLP */
                0x28 => {
                    self.plp();
                }

                /* ADC */
                0x69 | 0x65 | 0x75 | 0x6d | 0x7d | 0x79 | 0x61 | 0x71 => {
                    self.adc(&opcode.mode);
                }

                /* SBC */
                0xe9 | 0xe5 | 0xf5 | 0xed | 0xfd | 0xf9 | 0xe1 | 0xf1 => {
                    self.sbc(&opcode.mode);
                }

                /* AND */
                0x29 | 0x25 | 0x35 | 0x2d | 0x3d | 0x39 | 0x21 | 0x31 => {
                    self.and(&opcode.mode);
                }

                /* EOR */
                0x49 | 0x45 | 0x55 | 0x4d | 0x5d | 0x59 | 0x41 | 0x51 => {
                    self.eor(&opcode.mode);
                }

                /* ORA */
                0x09 | 0x05 | 0x15 | 0x0d | 0x1d | 0x19 | 0x01 | 0x11 => {
                    self.ora(&opcode.mode);
                }

                /* LSR */ 0x4a => self.lsr_accumulator(),

                /* LSR */
                0x46 | 0x56 | 0x4e | 0x5e => {
                    self.lsr(&opcode.mode);
                }

                /*ASL*/ 0x0a => self.asl_accumulator(),

                /* ASL */
                0x06 | 0x16 | 0x0e | 0x1e => {
                    self.asl(&opcode.mode);
                }
                /*ROL*/ 0x2a => self.rol_accumulator(),
                /* ROL */
                0x26 | 0x36 | 0x2e | 0x3e => {
                    self.rol(&opcode.mode);
                }
                /* ROR */ 0x6a => self.ror_accumulator(),
                /* ROR */
                0x66 | 0x76 | 0x6e | 0x7e => {
                    self.ror(&opcode.mode);
                }
                /* INC */
                0xe6 | 0xf6 | 0xee | 0xfe => {
                    self.inc(&opcode.mode);
                }
                /* INY */
                0xc8 => self.iny(),
                /* DEC */
                0xc6 | 0xd6 | 0xce | 0xde => {
                    self.dec(&opcode.mode);
                }
                /* DEX */
                0xca => {
                    self.dex();
                }
                /* DEY */
                0x88 => {
                    self.dey();
                }
                /* CMP */
                0xc9 | 0xc5 | 0xd5 | 0xcd | 0xdd | 0xd9 | 0xc1 | 0xd1 => {
                    self.compare(&opcode.mode, self.register_a);
                }
                /* CPY */
                0xc0 | 0xc4 | 0xcc => {
                    self.compare(&opcode.mode, self.register_y);
                }
                /* CPX */
                0xe0 | 0xe4 | 0xec => self.compare(&opcode.mode, self.register_x),
                /* JMP Absolute */
                0x4c => {
                    let mem_address = self.mem_read_u16(self.program_counter);
                    self.program_counter = mem_address;
                }
                /* JMP Indirect */
                0x6c => {
                    let mem_address = self.mem_read_u16(self.program_counter);
                    // let indirect_ref = self.mem_read_u16(mem_address);
                    //6502 bug mode with with page boundary:
                    //  if address $3000 contains $40, $30FF contains $80, and $3100 contains $50,
                    // the result of JMP ($30FF) will be a transfer of control to $4080 rather than $5080 as you intended
                    // i.e. the 6502 took the low byte of the address from $30FF and the high byte from $3000

                    let indirect_ref = if mem_address & 0x00FF == 0x00FF {
                        let lo = self.mem_read(mem_address);
                        let hi = self.mem_read(mem_address & 0xFF00);
                        (hi as u16) << 8 | (lo as u16)
                    } else {
                        self.mem_read_u16(mem_address)
                    };

                    self.program_counter = indirect_ref;
                }
                /* JSR */
                0x20 => {
                    self.stack_push_u16(self.program_counter + 2 - 1);
                    let target_address = self.mem_read_u16(self.program_counter);
                    self.program_counter = target_address
                }
                /* RTS */
                0x60 => {
                    self.program_counter = self.stack_pop_u16() + 1;
                }
                /* RTI */
                0x40 => {
                    self.status = self.stack_pop();
                    self.clear_flag(CpuFlags::BREAK);
                    self.set_flag(CpuFlags::BREAK2);

                    self.program_counter = self.stack_pop_u16();
                }
                /* BNE */
                0xd0 => {
                    self.branch(!self.contain_flag(CpuFlags::ZERO));
                }
                /* BVS */
                0x70 => {
                    self.branch(self.contain_flag(CpuFlags::OVERFLOW));
                }
                /* BVC */
                0x50 => {
                    self.branch(!self.contain_flag(CpuFlags::OVERFLOW));
                }
                /* BPL */
                0x10 => {
                    self.branch(!self.contain_flag(CpuFlags::NEGATIVE));
                }
                /* BMI */
                0x30 => {
                    self.branch(self.contain_flag(CpuFlags::NEGATIVE));
                }
                /* BEQ */
                0xf0 => {
                    self.branch(self.contain_flag(CpuFlags::NEGATIVE));
                }
                /* BCS */
                0xb0 => {
                    self.branch(self.contain_flag(CpuFlags::CARRY));
                }
                /* BCC */
                0x90 => {
                    self.branch(!self.contain_flag(CpuFlags::CARRY));
                }
                /* BIT */
                0x24 | 0x2c => {
                    self.bit(&opcode.mode);
                }
                /* STA */
                0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 => {
                    self.sta(&opcode.mode);
                }
                /* STX */
                0x86 | 0x96 | 0x8e => {
                    let addr = self.get_operand_address(&opcode.mode);
                    self.mem_write(addr, self.register_x);
                }
                /* STY */
                0x84 | 0x94 | 0x8c => {
                    let addr = self.get_operand_address(&opcode.mode);
                    self.mem_write(addr, self.register_y);
                }
                /* LDX */
                0xa2 | 0xa6 | 0xb6 | 0xae | 0xbe => {
                    self.ldx(&opcode.mode);
                }
                /* LDY */
                0xa0 | 0xa4 | 0xb4 | 0xac | 0xbc => {
                    self.ldy(&opcode.mode);
                }
                /* NOP */
                0xea => {
                    //do nothing
                }
                /* TAY */
                0xa8 => {
                    self.register_y = self.register_a;
                    self.update_zero_and_negative_flags(self.register_y);
                }
                /* TSX */
                0xba => {
                    self.register_x = self.stack_pointer;
                    self.update_zero_and_negative_flags(self.register_x);
                }
                /* TXA */
                0x8a => {
                    self.register_a = self.register_x;
                    self.update_zero_and_negative_flags(self.register_a);
                }
                /* TXS */
                0x9a => {
                    self.stack_pointer = self.register_x;
                }
                /* TYA */
                0x98 => {
                    self.register_a = self.register_y;
                    self.update_zero_and_negative_flags(self.register_a);
                }


                _=>todo!()
            }

            if program_counter_state==self.program_counter{
                self.program_counter+=(opcode.len-1) as u16;
            }
        }
    }
}


#[cfg(test)]
mod tests{
    use super::*;

    #[test]
    fn test_0xa9_load_data(){
        let mut cpu=CPU::new();
        cpu.load_and_run(vec![0xa9,0x05,0x00]);
        assert_eq!(cpu.register_a,0x05);
        assert!(cpu.status&0b0000_0010==0b00);
        assert!(cpu.status&0b1000_0000==0);
    }

    #[test]
    fn test_0xa9_zero_flag(){
        let mut cpu=CPU::new();
        cpu.load_and_run(vec![0xa9,0x00,0x00]);
        assert!(cpu.status&0b0000_0010==0b10);
    }
    
    #[test]
    fn test_0xaa_tax_move_a_to_x() {
        let mut cpu = CPU::new();
        cpu.register_a = 10;

        cpu.load_and_run(vec![0xaa, 0x00]);

        assert_eq!(cpu.register_x, 10)
    }

    #[test]
    fn test_5_ops_working_together() {
        let mut cpu = CPU::new();

        cpu.load_and_run(vec![0xa9, 0xc0, 0xaa, 0xe8, 0x00]);

        assert_eq!(cpu.register_x, 0xc1)
    }

    #[test]
    fn test_inx_overflow() {
        let mut cpu = CPU::new();
        cpu.register_x = 0xff;

        cpu.load_and_run(vec![0xe8, 0xe8, 0x00]);

        assert_eq!(cpu.register_x, 1)
    }

    #[test]
    fn test_lda_from_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0x55);

        cpu.load_and_run(vec![0xa5, 0x10, 0x00]);

        assert_eq!(cpu.register_a, 0x55);
    }
    
    #[test]
    fn test_lda_from_memory_indirect_x() {
        let mut cpu = CPU::new();
        cpu.mem_write_u16(0x18, 0xFF05);
        cpu.mem_write(0xFF05, 0x5A);
        cpu.register_x = 0x08;
        cpu.load_and_run(vec![0xa1, 0x10, 0x00]);
        assert_eq!(cpu.register_a, 0x5A);
    }

    #[test]
    fn test_lda_from_memory_indirect_y() {
        let mut cpu = CPU::new();
        cpu.mem_write_u16(0x10, 0xFF06);
        cpu.mem_write(0xFF09, 0x5B);
        cpu.register_y = 0x03;
        cpu.load_and_run(vec![0xb1, 0x10, 0x00]);
        assert_eq!(cpu.register_a, 0x5B);
    }

    
}

