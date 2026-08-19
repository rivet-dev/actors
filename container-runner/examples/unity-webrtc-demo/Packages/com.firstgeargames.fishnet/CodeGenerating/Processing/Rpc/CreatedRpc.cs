using FishNet.Object.Helping;
using MonoFN.Cecil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Processing.Rpc
{
    internal class CreatedRpc
    {
        public MethodDefinition OriginalMethodDef;
        public uint MethodHash;
        public AttributeData AttributeData;
        public MethodDefinition WriterMethodDef;
        public MethodDefinition ReaderMethodDef;
        public MethodDefinition LogicMethodDef;
        public MethodDefinition RedirectMethodDef;
        public bool RunLocally;
        public RpcType RpcType => AttributeData.RpcType;
        public CustomAttribute Attribute => AttributeData.Attribute;
        public TypeDefinition TypeDef => OriginalMethodDef.DeclaringType;
        public ModuleDefinition Module => OriginalMethodDef.Module;
    }

    internal static class CreatedRpcExtensions
    {
        /// <summary>
        /// Returns CreatedRpc for rpcType.
        /// </summary>
        /// <returns></returns>
        public static CreatedRpc GetCreatedRpc(this List<CreatedRpc> lst, RpcType rpcType)
        {
            for (int i = 0; i < lst.Count; i++)
            {
                if (lst[i].RpcType == rpcType)
                    return lst[i];
            }
            // Fall through.
            return null;
        }

        /// <summary>
        /// Returns combined RpcType for all entries.
        /// </summary>
        /// <returns></returns>
        public static RpcType GetCombinedRpcType(this List<CreatedRpc> lst)
        {
            RpcType result = RpcType.None;
            for (int i = 0; i < lst.Count; i++)
                result |= lst[i].RpcType;

            return result;
        }
    }
}