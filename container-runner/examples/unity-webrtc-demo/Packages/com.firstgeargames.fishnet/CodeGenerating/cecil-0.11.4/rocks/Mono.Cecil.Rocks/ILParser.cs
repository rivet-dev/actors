//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.Cil;
using MonoFN.Collections.Generic;
using System;

namespace MonoFN.Cecil.Rocks
{
#if UNITY_EDITOR
    public
#endif
        interface IILVisitor
    {
        void OnInlineNone(OpCode opcode);
        void OnInlineSByte(OpCode opcode, sbyte value);
        void OnInlineByte(OpCode opcode, byte value);
        void OnInlineInt32(OpCode opcode, int value);
        void OnInlineInt64(OpCode opcode, long value);
        void OnInlineSingle(OpCode opcode, float value);
        void OnInlineDouble(OpCode opcode, double value);
        void OnInlineString(OpCode opcode, string value);
        void OnInlineBranch(OpCode opcode, int offset);
        void OnInlineSwitch(OpCode opcode, int[] offsets);
        void OnInlineVariable(OpCode opcode, VariableDefinition variable);
        void OnInlineArgument(OpCode opcode, ParameterDefinition parameter);
        void OnInlineSignature(OpCode opcode, CallSite callSite);
        void OnInlineType(OpCode opcode, TypeReference type);
        void OnInlineField(OpCode opcode, FieldReference field);
        void OnInlineMethod(OpCode opcode, MethodReference method);
    }

#if UNITY_EDITOR
    public
#endif
        static class ILParser
    {
        private class ParseContext
        {
            public CodeReader Code { get; set; }
            public int Position { get; set; }
            public MetadataReader Metadata { get; set; }
            public Collection<VariableDefinition> Variables { get; set; }
            public IILVisitor Visitor { get; set; }
        }

        public static void Parse(MethodDefinition method, IILVisitor visitor)
        {
            if (method == null)
                throw new ArgumentNullException("method");
            if (visitor == null)
                throw new ArgumentNullException("visitor");
            if (!method.HasBody || !method.HasImage)
                throw new ArgumentException();

            method.Module.Read(method, (m, _) =>
            {
                ParseMethod(m, visitor);
                return true;
            });
        }

        private static void ParseMethod(MethodDefinition method, IILVisitor visitor)
        {
            ParseContext context = CreateContext(method, visitor);
            CodeReader code = context.Code;

            byte flags = code.ReadByte();

            switch (flags & 0x3)
            {
                case 0x2: // tiny
                    int code_size = flags >> 2;
                    ParseCode(code_size, context);
                    break;
                case 0x3: // fat
                    code.Advance(-1);
                    ParseFatMethod(context);
                    break;
                default:
                    throw new NotSupportedException();
            }

            code.MoveBackTo(context.Position);
        }

        private static ParseContext CreateContext(MethodDefinition method, IILVisitor visitor)
        {
            CodeReader code = method.Module.Read(method, (_, reader) => reader.code);
            int position = code.MoveTo(method);

            return new()
            {
                Code = code,
                Position = position,
                Metadata = code.reader,
                Visitor = visitor
            };
        }

        private static void ParseFatMethod(ParseContext context)
        {
            CodeReader code = context.Code;

            code.Advance(4);
            int code_size = code.ReadInt32();
            MetadataToken local_var_token = code.ReadToken();

            if (local_var_token != MetadataToken.Zero)
                context.Variables = code.ReadVariables(local_var_token);

            ParseCode(code_size, context);
        }

        private static void ParseCode(int code_size, ParseContext context)
        {
            CodeReader code = context.Code;
            MetadataReader metadata = context.Metadata;
            IILVisitor visitor = context.Visitor;

            int start = code.Position;
            int end = start + code_size;

            while (code.Position < end)
            {
                byte il_opcode = code.ReadByte();
                OpCode opcode = il_opcode != 0xfe ? OpCodes.OneByteOpCode[il_opcode] : OpCodes.TwoBytesOpCode[code.ReadByte()];

                switch (opcode.OperandType)
                {
                    case OperandType.InlineNone:
                        visitor.OnInlineNone(opcode);
                        break;
                    case OperandType.InlineSwitch:
                        int length = code.ReadInt32();
                        int[] branches = new int [length];
                        for (int i = 0; i < length; i++)
                            branches[i] = code.ReadInt32();
                        visitor.OnInlineSwitch(opcode, branches);
                        break;
                    case OperandType.ShortInlineBrTarget:
                        visitor.OnInlineBranch(opcode, code.ReadSByte());
                        break;
                    case OperandType.InlineBrTarget:
                        visitor.OnInlineBranch(opcode, code.ReadInt32());
                        break;
                    case OperandType.ShortInlineI:
                        if (opcode == OpCodes.Ldc_I4_S)
                            visitor.OnInlineSByte(opcode, code.ReadSByte());
                        else
                            visitor.OnInlineByte(opcode, code.ReadByte());
                        break;
                    case OperandType.InlineI:
                        visitor.OnInlineInt32(opcode, code.ReadInt32());
                        break;
                    case OperandType.InlineI8:
                        visitor.OnInlineInt64(opcode, code.ReadInt64());
                        break;
                    case OperandType.ShortInlineR:
                        visitor.OnInlineSingle(opcode, code.ReadSingle());
                        break;
                    case OperandType.InlineR:
                        visitor.OnInlineDouble(opcode, code.ReadDouble());
                        break;
                    case OperandType.InlineSig:
                        visitor.OnInlineSignature(opcode, code.GetCallSite(code.ReadToken()));
                        break;
                    case OperandType.InlineString:
                        visitor.OnInlineString(opcode, code.GetString(code.ReadToken()));
                        break;
                    case OperandType.ShortInlineArg:
                        visitor.OnInlineArgument(opcode, code.GetParameter(code.ReadByte()));
                        break;
                    case OperandType.InlineArg:
                        visitor.OnInlineArgument(opcode, code.GetParameter(code.ReadInt16()));
                        break;
                    case OperandType.ShortInlineVar:
                        visitor.OnInlineVariable(opcode, GetVariable(context, code.ReadByte()));
                        break;
                    case OperandType.InlineVar:
                        visitor.OnInlineVariable(opcode, GetVariable(context, code.ReadInt16()));
                        break;
                    case OperandType.InlineTok:
                    case OperandType.InlineField:
                    case OperandType.InlineMethod:
                    case OperandType.InlineType:
                        IMetadataTokenProvider member = metadata.LookupToken(code.ReadToken());
                        switch (member.MetadataToken.TokenType)
                        {
                            case TokenType.TypeDef:
                            case TokenType.TypeRef:
                            case TokenType.TypeSpec:
                                visitor.OnInlineType(opcode, (TypeReference)member);
                                break;
                            case TokenType.Method:
                            case TokenType.MethodSpec:
                                visitor.OnInlineMethod(opcode, (MethodReference)member);
                                break;
                            case TokenType.Field:
                                visitor.OnInlineField(opcode, (FieldReference)member);
                                break;
                            case TokenType.MemberRef:
                                FieldReference field_ref = member as FieldReference;
                                if (field_ref != null)
                                {
                                    visitor.OnInlineField(opcode, field_ref);
                                    break;
                                }

                                MethodReference method_ref = member as MethodReference;
                                if (method_ref != null)
                                {
                                    visitor.OnInlineMethod(opcode, method_ref);
                                    break;
                                }

                                throw new InvalidOperationException();
                        }
                        break;
                }
            }
        }

        private static VariableDefinition GetVariable(ParseContext context, int index)
        {
            return context.Variables[index];
        }
    }
}