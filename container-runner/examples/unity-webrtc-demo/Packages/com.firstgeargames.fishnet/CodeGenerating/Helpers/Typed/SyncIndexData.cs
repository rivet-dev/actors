using MonoFN.Cecil.Cil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Helping
{
    /// <summary>
    /// Data used to modify an RpcIndex should the class have to be rebuilt.
    /// </summary>
    internal class SyncIndexData
    {
        public uint SyncCount = 0;
        public List<Instruction> DelegateInstructions = new();
    }
}