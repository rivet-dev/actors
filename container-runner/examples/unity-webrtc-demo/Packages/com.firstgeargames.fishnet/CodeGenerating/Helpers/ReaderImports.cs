using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Connection;
using FishNet.Serializing;
using MonoFN.Cecil;
using System;
using System.Reflection;

namespace FishNet.CodeGenerating.Helping
{
    internal class ReaderImports : CodegenBase
    {
        #region Reflection references.
        public TypeReference PooledReader_TypeRef;
        public TypeReference Reader_TypeRef;
        public TypeReference NetworkConnection_TypeRef;
        public MethodReference PooledReader_ReadNetworkBehaviour_MethodRef;
        public MethodReference Reader_ReadPackedWhole_MethodRef;
        public MethodReference Reader_ReadDictionary_MethodRef;
        public MethodReference Reader_ReadList_MethodRef;
        public MethodReference Reader_ReadHashSet_MethodRef;
        public MethodReference Reader_ReadArray_MethodRef;
        public TypeReference GenericReader_TypeRef;
        public MethodReference GenericReader_Read_MethodRef;
        #endregion

        /// <summary>
        /// Imports references needed by this helper.
        /// </summary>
        /// <param name = "moduleDef"></param>
        /// <returns></returns>
        public override bool ImportReferences()
        {
            ReaderProcessor rp = GetClass<ReaderProcessor>();

            PooledReader_TypeRef = ImportReference(typeof(PooledReader));
            Reader_TypeRef = ImportReference(typeof(Reader));
            NetworkConnection_TypeRef = ImportReference(typeof(NetworkConnection));
            GenericReader_TypeRef = ImportReference(typeof(GenericReader<>));

            TypeDefinition genericWriterTd = GenericReader_TypeRef.CachedResolve(Session);
            GenericReader_Read_MethodRef = ImportReference(genericWriterTd.GetMethod(nameof(GenericReader<int>.SetRead)));

            Type pooledReaderType = typeof(PooledReader);
            foreach (MethodInfo methodInfo in pooledReaderType.GetMethods())
            {
                int parameterCount = methodInfo.GetParameters().Length;
                /* Special methods. */
                if (methodInfo.Name == nameof(PooledReader.ReadUnsignedPackedWhole))
                    Reader_ReadPackedWhole_MethodRef = ImportReference(methodInfo);
                // Relay readers.
                else if (parameterCount == 0 && methodInfo.Name == nameof(PooledReader.ReadDictionary))
                    Reader_ReadDictionary_MethodRef = ImportReference(methodInfo);
                else if (parameterCount == 0 && methodInfo.Name == nameof(PooledReader.ReadList))
                    Reader_ReadList_MethodRef = ImportReference(methodInfo);
                else if (parameterCount == 0 && methodInfo.Name == nameof(PooledReader.ReadHashSet))
                    Reader_ReadHashSet_MethodRef = ImportReference(methodInfo);
                else if (parameterCount == 0 && methodInfo.Name == nameof(PooledReader.ReadArrayAllocated))
                    Reader_ReadArray_MethodRef = ImportReference(methodInfo);
            }

            return true;
        }
    }
}