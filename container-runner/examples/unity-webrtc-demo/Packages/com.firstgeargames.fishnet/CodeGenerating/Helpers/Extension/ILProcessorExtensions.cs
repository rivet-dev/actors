using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class ILProcessorExtensions
    {
        /// <summary>
        /// Creates a debug log for text without any conditions.
        /// </summary>
        public static List<Instruction> DebugLog(this ILProcessor processor, CodegenSession session, string txt)
        {
            List<Instruction> insts = new();
            insts.Add(processor.Create(OpCodes.Ldstr, txt));
            insts.Add(processor.Create(OpCodes.Call, session.GetClass<GeneralHelper>().Debug_LogCommon_MethodRef));
            return insts;
        }

        /// <summary>
        /// Creates a debug log for vd without any conditions.
        /// </summary>
        public static List<Instruction> DebugLog(this ILProcessor processor, CodegenSession session, VariableDefinition vd)
        {
            List<Instruction> insts = new();
            insts.Add(processor.Create(OpCodes.Ldloc, vd));
            insts.Add(processor.Create(OpCodes.Box, vd.VariableType));
            insts.Add(processor.Create(OpCodes.Call, session.GetClass<GeneralHelper>().Debug_LogCommon_MethodRef));
            return insts;
        }

        /// <summary>
        /// Creates a debug log for vd without any conditions.
        /// </summary>
        public static List<Instruction> DebugLog(this ILProcessor processor, CodegenSession session, FieldDefinition fd, bool loadArg0)
        {
            List<Instruction> insts = new();
            if (loadArg0)
                insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Ldfld, fd));
            insts.Add(processor.Create(OpCodes.Box, fd.FieldType));
            insts.Add(processor.Create(OpCodes.Call, session.GetClass<GeneralHelper>().Debug_LogCommon_MethodRef));
            return insts;
        }

        /// <summary>
        /// Creates a debug log for pd without any conditions.
        /// </summary>
        public static List<Instruction> DebugLog(this ILProcessor processor, CodegenSession session, ParameterDefinition pd)
        {
            List<Instruction> insts = new();
            insts.Add(processor.Create(OpCodes.Ldloc, pd));
            insts.Add(processor.Create(OpCodes.Box, pd.ParameterType));
            insts.Add(processor.Create(OpCodes.Call, session.GetClass<GeneralHelper>().Debug_LogCommon_MethodRef));
            return insts;
        }

        /// <summary>
        /// Removes Ret if the last instruction.
        /// </summary>
        /// <returns>True if ret was removed.</returns>
        public static bool RemoveEndRet(this ILProcessor processor)
        {
            int instCount = processor.Body.Instructions.Count;
            if (instCount == 0)
                return false;

            if (processor.Body.Instructions[instCount - 1].OpCode == OpCodes.Ret)
            {
                processor.Body.Instructions.RemoveAt(instCount - 1);
                return true;
            }
            else
            {
                return false;
            }
        }

        ///// <summary>
        ///// Creates a debug log for mr without any conditions.
        ///// </summary>
        // public static void DebugLog(this ILProcessor processor, MethodReference mr)
        // {
        //    processor.Emit(OpCodes.Call, mr);
        //    processor.Emit(OpCodes.Box, mr.ReturnType);
        //    processor.Emit(OpCodes.Call, base.GetClass<GeneralHelper>().Debug_LogCommon_MethodRef);
        // }

        /// <summary>
        /// Inserts instructions at the beginning.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void InsertAt(this ILProcessor processor, int target, List<Instruction> instructions)
        {
            for (int i = 0; i < instructions.Count; i++)
                processor.Body.Instructions.Insert(i + target, instructions[i]);
        }

        /// <summary>
        /// Inserts instructions at the beginning.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void InsertFirst(this ILProcessor processor, List<Instruction> instructions)
        {
            for (int i = 0; i < instructions.Count; i++)
                processor.Body.Instructions.Insert(i, instructions[i]);
        }

        /// <summary>
        /// Inserts instructions at the end while also moving Ret down.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void InsertLast(this ILProcessor processor, List<Instruction> instructions)
        {
            bool retRemoved = false;
            int startingCount = processor.Body.Instructions.Count;
            // Remove ret if it exist and add it back in later.
            if (startingCount > 0)
            {
                if (processor.Body.Instructions[startingCount - 1].OpCode == OpCodes.Ret)
                {
                    processor.Body.Instructions.RemoveAt(startingCount - 1);
                    retRemoved = true;
                }
            }

            foreach (Instruction inst in instructions)
                processor.Append(inst);

            // Add ret back if it was removed.
            if (retRemoved)
                processor.Emit(OpCodes.Ret);
        }

        /// <summary>
        /// Inserts instructions before target.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void InsertBefore(this ILProcessor processor, Instruction target, List<Instruction> instructions)
        {
            int index = processor.Body.Instructions.IndexOf(target);
            for (int i = 0; i < instructions.Count; i++)
                processor.Body.Instructions.Insert(index + i, instructions[i]);
        }

        /// <summary>
        /// Adds instructions to the end of processor.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void Add(this ILProcessor processor, List<Instruction> instructions)
        {
            for (int i = 0; i < instructions.Count; i++)
                processor.Body.Instructions.Add(instructions[i]);
        }

        /// <summary>
        /// Inserts instructions before returns. Only works on void types.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "instructions"></param>
        public static void InsertBeforeReturns(this ILProcessor processor, CodegenSession session, List<Instruction> instructions)
        {
            if (processor.Body.Method.ReturnType.FullName != session.Module.TypeSystem.Void.FullName)
            {
                session.LogError($"Cannot insert instructions before returns on {processor.Body.Method.FullName} because it does not return void.");
                return;
            }

            /* Insert at the end of the method
             * and get the first instruction that was inserted.
             * Any returns or breaks which would exit the method
             * will jump to this instruction instead. */
            processor.InsertLast(instructions);
            Instruction startInst = processor.Body.Instructions[processor.Body.Instructions.Count - instructions.Count];

            // Look for anything that jumps to rets.
            for (int i = 0; i < processor.Body.Instructions.Count; i++)
            {
                Instruction inst = processor.Body.Instructions[i];
                if (inst.Operand is Instruction operInst)
                {
                    if (operInst.OpCode == OpCodes.Ret)
                        inst.Operand = startInst;
                }
            }
        }
    }
}