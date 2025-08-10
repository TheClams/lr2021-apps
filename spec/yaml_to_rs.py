#!/usr/bin/env python3

import yaml
import sys
from typing import Any
from dataclasses import dataclass
from pathlib import Path

import re

def to_snake(s: str) -> str:
    # Add an underscore before each uppercase letter that is followed by a lowercase letter
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', s)
    # Add an underscore before each lowercase letter that is preceded by an uppercase letter
    s = re.sub(r'([a-z\d])([A-Z])', r'\1_\2', s)
    # Convert the entire string to lowercase
    s = s.lower()
    return s

def snake_to_pascal(snake_str: str) -> str:
    """Convert snake_case to PascalCase"""
    # Replace _ between two number by p (like point)
    s = re.sub(r'(\d+)_(\d+)', r'\1p\2', snake_str)
    return ''.join(word.capitalize() for word in s.split('_'))

enum_remap : dict[str,str] = {
    'BitrateCh1' : 'ZwaveMode',
    'BitrateCh2' : 'ZwaveMode',
    'BitrateCh3' : 'ZwaveMode',
    'BitrateCh4' : 'ZwaveMode',
    'LastDetect' : 'ZwaveMode',
    'Sd1Sf' : 'Sf',
    'Sd2Sf' : 'Sf',
    'Sd3Sf' : 'Sf',
    'Sd1Ldro' : 'Ldro',
    'Sd2Ldro' : 'Ldro',
    'Sd3Ldro' : 'Ldro',
}

@dataclass
class BytePosition:
    byte_index: int
    bit_range: str
    
    def get_bit_range_tuple(self) -> tuple[int, int]:
        """Parse bit range like '7:0' into (msb, lsb) tuple"""
        if ':' in self.bit_range:
            msb, lsb = map(int, self.bit_range.split(':'))
            return (msb, lsb)
        else:
            # Single bit
            bit = int(self.bit_range)
            return (bit, bit)

@dataclass
class Field:
    name: str
    bit_width: int
    signed: bool
    byte_positions: list[BytePosition]
    description: str
    optional: bool = False
    enum: dict[str, int] | None = None

@dataclass
class Command:
    name: str
    opcode: int
    description: str
    parameters: list[Field]
    status_fields: list[Field]

class ValidationError(Exception):
    pass

def parse_byte_positions(positions_data: list[tuple[int,str]]) -> list[BytePosition]:
    """Parse byte positions from YAML format"""
    positions : list[BytePosition] = []
    for pos_data in positions_data:
        if len(pos_data) != 2:
            raise ValidationError(f"Invalid byte position format: {pos_data}")
        byte_index, bit_range = pos_data
        positions.append(BytePosition(byte_index, str(bit_range)))
    return positions

def validate_field(field: Field, context: str) -> None:
    """Validate field consistency"""
    if field.bit_width < 0:
        raise ValidationError(f"{context}: bit_width cannot be negative for field '{field.name}'")
    
    if field.bit_width == 0 and field.byte_positions:
        raise ValidationError(f"{context}: variable length field '{field.name}' should not have byte_positions")
    
    if field.bit_width > 0:
        # Calculate total bits from byte positions
        total_bits = 0
        for pos in field.byte_positions:
            msb, lsb = pos.get_bit_range_tuple()
            if msb < lsb:
                raise ValidationError(f"{context}: invalid bit range '{pos.bit_range}' for field '{field.name}' (MSB < LSB)")
            total_bits += (msb - lsb + 1)
        
        if total_bits != field.bit_width:
            raise ValidationError(f"{context}: bit_width ({field.bit_width}) doesn't match byte_positions total ({total_bits}) for field '{field.name}'")

def parse_field(field_data: dict[str, Any], context: str) -> Field:  # pyright: ignore[reportExplicitAny]
    """Parse a field from YAML data"""
    try:
        name : str = field_data['name']  # pyright: ignore[reportAny]
        bit_width : int = field_data['bit_width']  # pyright: ignore[reportAny]
        byte_positions = parse_byte_positions(field_data.get('byte_positions', []))  # pyright: ignore[reportAny]
        signed : bool = field_data.get('signed', False)  # pyright: ignore[reportAny]
        description : str = field_data['description']  # pyright: ignore[reportAny]
        optional : bool = field_data.get('optional', False)  # pyright: ignore[reportAny]
        enum = field_data.get('enum')
        
        field = Field(name, bit_width, signed, byte_positions, description, optional, enum)
        validate_field(field, f"{context}.{name}")
        return field
        
    except KeyError as e:
        raise ValidationError(f"{context}: missing required field property: {e}")
    except Exception as e:
        raise ValidationError(f"{context}: error parsing field: {e}")

def parse_command(cmd_name: str, cmd_data: dict[str, Any]) -> Command:  # pyright: ignore[reportExplicitAny]
    """Parse a command from YAML data"""
    try:
        opcode : int = cmd_data['opcode']  # pyright: ignore[reportAny]
        description : str = cmd_data['description']  # pyright: ignore[reportAny]
        
        parameters : list[Field] = []
        for param_data in cmd_data.get('parameters', []): # pyright: ignore[reportAny]
            param = parse_field(param_data, f"command '{cmd_name}' parameter") # pyright: ignore[reportAny]
            parameters.append(param)
        
        status_fields : list[Field] = []
        for field_data in cmd_data.get('status_fields', []): # pyright: ignore[reportAny]
            field = parse_field(field_data, f"command '{cmd_name}' status_field") # pyright: ignore[reportAny]
            status_fields.append(field)
        
        return Command(cmd_name, opcode, description, parameters, status_fields)
        
    except KeyError as e:
        raise ValidationError(f"command '{cmd_name}': missing required property: {e}")
    except Exception as e:
        raise ValidationError(f"command '{cmd_name}': {e}")

def get_rust_type(field: Field) -> str:
    """Get appropriate Rust type for a field"""
    if field.enum:
        t = snake_to_pascal(field.name)
        return enum_remap.get(t,t)
    elif field.bit_width == 0:
        return "&[u8]"  # Variable length
    elif field.bit_width == 1:
        return "bool"
    elif field.bit_width <= 8:
        return "i8" if field.signed else "u8"
    elif field.bit_width <= 16:
        return "i16" if field.signed else "u16"
    elif field.bit_width <= 32:
        return "i32" if field.signed else "u32"
    else:
        return "i64" if field.signed else "u64"

def gen_enum(field: Field) -> str:
    """Generate Rust enum for a field"""
    if not field.enum:
        return ''
    enum_name = snake_to_pascal(field.name)
    lines = ["",f"/// {field.description}"]
    lines.append("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    lines.append(f"pub enum {enum_name} {{")

    for variant_name, value in field.enum.items():
        name = snake_to_pascal(variant_name)
        # print(f'Enum {enum_name} : {variant_name} -> {name}')
        lines.append(f"    {name} = {value},")
    
    lines.append("}")
    # Add builder for some enum
    if enum_name == 'ZwaveMode':
        lines.append("impl ZwaveMode {")
        lines.append("    pub fn new(val: u8) -> Self{")
        lines.append("        match val {")
        lines.append("            3 => ZwaveMode::R3,")
        lines.append("            2 => ZwaveMode::R2,")
        lines.append("            _ => ZwaveMode::R1,")
        lines.append("        }")
        lines.append("    }")
        lines.append("}")
    return '\n'.join(lines)

def rsp_len(fields: list[Field]) -> int:
    if not fields:
        return 2  # Just status bytes
    
    max_byte = 1  # Start with status bytes (0, 1)
    for field in fields:
        for pos in field.byte_positions:
            max_byte = max(max_byte, pos.byte_index)
    
    return max_byte + 1

def gen_req(cmd: Command, _category: str, advanced: bool = False) -> str:
    """Generate Rust request function"""
    func_name = to_snake(cmd.name)
    
    if advanced:
        func_name += "_adv"

    func_suffix = "_req" if cmd.status_fields else "_cmd"
    func_name += func_suffix
    
    # Filter parameters based on advanced flag
    if advanced:
        params = cmd.parameters
    else:
        params = [p for p in cmd.parameters if not p.optional]
    
    # Skip variable length parameters for now
    skipped_params = [p for p in params if p.bit_width == 0]
    if skipped_params:
        return f"// TODO: Implement {func_name} (contains variable length parameters: {[p.name for p in skipped_params]})"
    
    # Generate function signature
    param_list: list[str] = []
    for param in params:
        # Skip the temp_format
        if param.name == 'temp_format':
            continue
        param_type = get_rust_type(param)
        param_list.append(f"{param.name}: {param_type}")
    
    # Calculate total request size
    opcode_size = 2
    param_size = sum((p.bit_width + 7) // 8 for p in params if p.bit_width > 0)
    total_size = opcode_size + param_size
    
    lines = [f"/// {cmd.description}"]
    if param_list:
        lines.append(f"pub fn {func_name}({', '.join(param_list)}) -> [u8; {total_size}] {{")
    else:
        lines.append(f"pub fn {func_name}() -> [u8; {total_size}] {{")
    
    # Generate opcode bytes
    opcode_msb = (cmd.opcode >> 8) & 0xFF
    opcode_lsb = cmd.opcode & 0xFF
    
    if not params:
        lines.append(f"    [0x{opcode_msb:02X}, 0x{opcode_lsb:02X}]")
    else:
        # Generate parameter packing code
        lines.append("    let mut cmd = [0u8; {}];".format(total_size))
        lines.append(f"    cmd[0] = 0x{opcode_msb:02X};")
        lines.append(f"    cmd[1] = 0x{opcode_lsb:02X};")
        lines.append("")
        
        for param in params:
            if param.bit_width == 0:
                continue

            # Generate bit packing for each byte position
            for pos in param.byte_positions:
                _, lsb = pos.get_bit_range_tuple()
                if param.bit_width == 1 and not param.enum:
                    lines.append(f"    if {param.name} {{ cmd[{pos.byte_index}] |= {1<<lsb}; }}")
                elif param.name == 'temp_format':
                    lines.append(f"    cmd[2] |= 8; // Force format to Celsius")
                else:
                    mask = (1 << param.bit_width) - 1 if param.bit_width < 8 else 255
                    shift_right = param.bit_width - sum((p_msb - p_lsb + 1) for p_pos in param.byte_positions[:param.byte_positions.index(pos)+1] for p_msb, p_lsb in [p_pos.get_bit_range_tuple()])
                    l = f"    cmd[{pos.byte_index}] |= "
                    if param.bit_width > 8:  l += '(';
                    if lsb!= 0: l += '(';
                    if shift_right!=0: l += '(';
                    if param.enum:
                        # print(f'Enum {param.name} : shift_right={shift_right}, lsb={lsb}, width={param.bit_width}')
                        if shift_right==0 and lsb==0 and param.bit_width==8:
                            l += f"{param.name} as u8"
                        else:
                            l += f"({param.name} as u8)"
                    else:
                        l += param.name
                    if shift_right!=0:
                        l += f" >> {shift_right})"
                    if param.bit_width!=8:
                        l += f" & 0x{mask:X}"
                    if lsb!= 0:
                        l += f') << {lsb}'
                    if param.bit_width > 8:
                        l += ') as u8';
                    l += ';'
                    lines.append(l)
        
        lines.append("    cmd")
    
    lines.append("}")
    return '\n'.join(lines)

def gen_rsp(cmd: Command, _category: str, advanced: bool = False) -> str:
    """Generate Rust response struct"""
    if not cmd.status_fields:
        return ""
    
    struct_name = f"{cmd.name}Rsp"
    if advanced:
        struct_name += "Adv"
    if struct_name.startswith("Get"):
        struct_name = struct_name[3:]
    
    # Filter fields based on advanced flag
    if advanced:
        fields = cmd.status_fields
    else:
        fields = [f for f in cmd.status_fields if not f.optional]
    
    buffer_size = rsp_len(fields)
    
    lines = [f"/// Response for {cmd.name} command"]
    lines.append("#[derive(Default)]")
    lines.append(f"pub struct {struct_name}([u8; {buffer_size}]);")
    lines.append("")
    lines.append(f"impl {struct_name} {{")
    lines.append("    /// Create a new response buffer")
    lines.append("    pub fn new() -> Self {")
    lines.append("        Self::default()")
    lines.append("    }")
    lines.append("")
    lines.append("    /// Return Status")
    lines.append("    pub fn status(&mut self) -> Status {")
    lines.append("        Status::from_slice(&self.0[..2])")
    lines.append("    }")

    # Generate accessor methods for each field
    for field in fields:
            
        if field.bit_width == 0:
            lines.append(f"    // TODO: Implement accessor for variable length field '{field.name}'")
            continue
        
        return_type = get_rust_type(field).replace('&[u8]', 'u32')  # Variable length becomes u32 for now
        
        lines.append(f"")
        lines.append(f"    /// {field.description}")

        # Custom implementation
        if cmd.name == 'GetStatus' and field.name=='intr':
            lines.append(f"    pub fn {field.name}(&self) -> Intr {{")
            lines.append('        Intr::from_slice(&self.0[2..6])')
            lines.append('    }')
            continue
        if cmd.name == 'GetZwavePacketStatus' and field.name=='last_detect':
            lines.append('    pub fn last_detect(&self) -> ZwaveMode {')
            lines.append('        ZwaveMode::new(self.0[6] & 0x3)')
            lines.append('    }')
            continue

        # Implementation
        lines.append(f"    pub fn {field.name}(&self) -> {return_type} {{")
        l = '        '
        
        if len(field.byte_positions) == 1 and field.bit_width <= 8:
            # Simple single byte case
            pos = field.byte_positions[0]
            msb, lsb = pos.get_bit_range_tuple()
            if field.signed and field.bit_width!=8:
                l+='('
            if field.bit_width==8:
                l += f"self.0[{pos.byte_index}]"
            elif lsb == 0:
                bit_count = msb - lsb + 1
                mask = (1 << bit_count) - 1
                l += f"self.0[{pos.byte_index}] & 0x{mask:X}"
            else:
                bit_count = msb - lsb + 1
                mask = (1 << bit_count) - 1
                l += f"(self.0[{pos.byte_index}] >> {lsb}) & 0x{mask:X}"
            if field.signed and field.bit_width!=8:
                l+=')'
            if return_type=='bool':
                l += ' != 0'
            elif field.signed:
                mask = 1 << msb
                l += f' as i8'
                if field.bit_width<8:
                    l += f' - if (self.0[{pos.byte_index}] & {mask:#0x}) != 0 {{1<<{field.bit_width}}} else {{0}}'

        else:
            # Multi-byte or complex bit extraction
            shift = 0
            raw_type = return_type.replace('i','u') if not field.enum else return_type

            if field.signed:
                l += 'let raw = '
            for (i,pos) in enumerate(reversed(field.byte_positions)):  # Process LSB first
                msb, lsb = pos.get_bit_range_tuple()

                bit_count = msb - lsb + 1
                has_mask = bit_count!=8 and msb!=7
                if shift != 0 : l += f'(';
                if has_mask   : l += f'(';
                if lsb!=0     : l += f'(';
                l += f"(self.0[{pos.byte_index}]"
                if lsb!=0:
                    l += f" >> {lsb})"
                if has_mask :
                    mask = (1 << bit_count) - 1
                    l += f" & 0x{mask:X})"
                l+= f' as {raw_type})'
                if shift != 0 :
                    l += f' << {shift})'
                shift += bit_count
                if i+1 != len(field.byte_positions):
                    l+= ' |\n        '
                    if field.signed: l+= '    ';
            if field.signed:
                bi = field.byte_positions[0].byte_index
                mask = 1 << field.byte_positions[0].get_bit_range_tuple()[0]
                l+= f';\n        raw as {return_type}'
                # No need to apply any offset when already aligned on a word boundary
                if field.bit_width!=16 and field.bit_width!=32 and field.bit_width!=64:
                    l+= f' - if (self.0[{bi}] & {mask:#0x}) != 0 {{1<<{field.bit_width}}} else {{0}}'
        lines.append(l)
        
        lines.append("    }")
    
    lines.append("}")
    lines.append("")
    lines.append(f"impl AsMut<[u8]> for {struct_name} {{")
    lines.append("    fn as_mut(&mut self) -> &mut [u8] {")
    lines.append("        &mut self.0")
    lines.append("    }")
    lines.append("}")

    if cmd.name == 'GetTemp':
        lines.append("impl defmt::Format for TempRsp {")
        lines.append("    fn format(&self, fmt: defmt::Formatter) {")
        lines.append("        defmt::write!(fmt, \"{}.{:02}\", self.0[2] as i8, (self.0[3] as u16 * 100) >> 8);")
        lines.append("    }")
        lines.append("}")
    elif cmd.name == 'GetVersion':
        lines.append("impl defmt::Format for VersionRsp {")
        lines.append("    fn format(&self, fmt: defmt::Formatter) {")
        lines.append("        defmt::write!(fmt, \"{:02x}.{:02x}\", self.major(), self.minor());")
        lines.append("    }")
        lines.append("}")


    
    return '\n'.join(lines)

def gen_file(category: str, commands: list[Command], output_dir: Path) -> None:
    """Generate complete Rust file for a category"""
    file_path = output_dir / f"cmd_{category}.rs"
    
    lines = [f"// {category.title()} commands API\n"]

    has_rsp = any(len(cmd.status_fields) > 0 for cmd in commands)

    if category=='system':
        lines.append("use crate::lr2021::status::{Status,Intr};")
    elif has_rsp:
        lines.append("use crate::lr2021::status::Status;")
    if category in ['ble', 'ook', 'zigbee', 'zwave']:
        lines.append("use super::RxBw;")
    
    # Collect all enums first
    enum_kind : dict[str,list[str]] = {}
    enums : list[str] = []
    skipped_cmd : list[str] = []
    
    for cmd in commands:
        # Check for variable length parameters
        has_var_length = any(p.bit_width == 0 for p in cmd.parameters)
        if has_var_length:
            skipped_cmd.append(cmd.name)
            continue
        
        for param in cmd.parameters:
            if param.enum:
                n = snake_to_pascal(param.name)
                if n in enum_remap.keys() or (n=='RxBw' and category!='fsk') or n=='TempFormat':
                    continue
                if n in enum_kind.keys():
                    if list(param.enum.keys()) != enum_kind[n]:
                        print(f'Conflicting type definition for {n} in {cmd.name}')
                    continue
                enum_kind[n] = list(param.enum.keys())
                enums.append(gen_enum(param))

        for status in cmd.status_fields:
            if status.enum:
                n = snake_to_pascal(status.name)
                if n in enum_remap.keys():
                    continue
                if n in enum_kind.keys():
                    if list(status.enum.keys()) != enum_kind[n]:
                        print(f'Conflicting type definition for {n} in {cmd.name}')
                    continue
                enum_kind[n] = list(status.enum.keys())
                enums.append(gen_enum(status))
    
    # Add enums
    if enums:
        lines.extend(enums)
        lines.append("")
    
    # Generate request functions
    for cmd in commands:
        has_var_length = any(p.bit_width == 0 for p in cmd.parameters)
        if has_var_length:
            continue
            
        has_optional = any(p.optional for p in cmd.parameters)
        
        # Generate base function
        lines.append(gen_req(cmd, category, advanced=False))
        lines.append("")
        
        # Generate advanced function if needed
        if has_optional:
            lines.append(gen_req(cmd, category, advanced=True))
            lines.append("")
    
    # Generate response structs
    if has_rsp:
        lines.append("// Response structs")
        lines.append("")
    
        for cmd in commands:
            if not cmd.status_fields:
                continue

            has_optional_status = any(f.optional for f in cmd.status_fields)

            # Generate base struct
            struct_code = gen_rsp(cmd, category, advanced=False)
            if struct_code:
                lines.append(struct_code)
                lines.append("")

            # Generate advanced struct if needed
            if has_optional_status:
                struct_code = gen_rsp(cmd, category, advanced=True)
                if struct_code:
                    lines.append(struct_code)
                    lines.append("")
    
    # Add summary of unimplemented commands
    if skipped_cmd:
        lines.append("// Commands with variable length parameters (not implemented):")
        for cmd_name in skipped_cmd:
            lines.append(f"// - {cmd_name}")
        lines.append("")
    
    # Write file
    with open(file_path, 'w') as f:
        _ = f.write('\n'.join(lines))

def main():
    
    yaml_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("./commands.yaml")
    output_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("../src/lr2021/cmd")
    
    # Create output directory if it doesn't exist
    output_dir.mkdir(parents=True, exist_ok=True)
    
    try:
        if yaml_path.is_file():
            # Single YAML file
            with open(yaml_path) as f:
                data = yaml.safe_load(f)  # pyright: ignore[reportAny]
            
            # Parse commands
            for category, category_data in data.get('categories', {}).items():  # pyright: ignore[reportAny]
                print(f'Category {category}')
                commands : list[Command] = []
                for cmd_name, cmd_data in category_data.get('commands', {}).items():  # pyright: ignore[reportAny]
                    try:
                        cmd = parse_command(cmd_name, cmd_data)  # pyright: ignore[reportAny]
                        commands.append(cmd)
                    except ValidationError as e:
                        print(f"Error in {yaml_path}:{cmd_name}: {e}", file=sys.stderr)
                        sys.exit(1)
            
                gen_file(category, commands, output_dir) # pyright: ignore[reportAny]

        else:
            print(f"Error: {yaml_path} is not a file or directory", file=sys.stderr)
            sys.exit(1)
            
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    
    print("Code generation completed successfully!")

if __name__ == "__main__":
    main()
