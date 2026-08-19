using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.ILCore;
using FishNet.CodeGenerating.Processing;
using FishNet.CodeGenerating.Processing.Rpc;
using MonoFN.Cecil;
using System.Collections.Generic;
using System.Linq;
using Unity.CompilationPipeline.Common.Diagnostics;
#if !UNITY_2020_1_OR_NEWER
using UnityEngine;
#endif
using SR = System.Reflection;

namespace FishNet.CodeGenerating
{
    internal class CodegenSession
    {
        /// <summary>
        /// Current module for this session.
        /// </summary>
        internal ModuleDefinition Module;
        /// <summary>
        /// Outputs errors when codegen fails.
        /// </summary>
        internal List<DiagnosticMessage> Diagnostics;
        /// <summary>
        /// SyncVars that are being accessed from an assembly other than the currently being processed one.
        /// </summary>
        internal List<FieldDefinition> DifferentAssemblySyncVars = new();
        /// <summary>
        /// CodegenBase classes for processing a module.
        /// </summary>
        private List<CodegenBase> _bases;
        /// <summary>
        /// Quick lookup of base classes.
        /// </summary>
        private Dictionary<string, CodegenBase> _basesCache = new();

        /// <summary>
        /// Returns class of type if found within CodegenBase classes.
        /// </summary>
        /// <typeparam name = "T"></typeparam>
        /// <returns></returns>
        internal T GetClass<T>() where T : CodegenBase
        {
            string tName = typeof(T).Name;
            return (T)_basesCache[tName];
        }

        /// <summary>
        /// Resets all helpers while importing any information needed by them.
        /// </summary>
        /// <param name = "module"></param>
        /// <returns></returns>
        internal bool Initialize(ModuleDefinition module)
        {
            Module = module;
            Diagnostics = new();

            _bases = new()
            {
                new ReaderImports(), new ReaderProcessor(), new WriterImports(), new WriterProcessor(), new PhysicsHelper(), new TimeManagerHelper(), new AttributeHelper(), new GeneralHelper(), new ObjectHelper(), new NetworkBehaviourHelper(), new TransportHelper(), new NetworkConnectionImports(), new PredictedObjectHelper(), new GeneratorHelper(), new CustomSerializerProcessor(), new NetworkBehaviourProcessor(), new QolAttributeProcessor(), new RpcProcessor(), new SyncTypeProcessor(), new PredictionProcessor()
            };

            // Add all to dictionary first, then import.
            foreach (CodegenBase item in _bases)
            {
                string tName = item.GetType().Name;
                _basesCache.Add(tName, item);
            }

            // Initialize.
            foreach (CodegenBase item in _bases)
            {
                item.Initialize(this);
                if (!item.ImportReferences())
                    return false;
            }

            return true;
        }

        #region Logging.
        public string MethodDefinitionTraceText(MethodDefinition methodDef) => methodDef == null ? " Null MethodDef" : $" TypeDef [{methodDef.DeclaringType.FullName}] Method [{methodDef.Name}]";
        public string TypeDefinitionTraceText(TypeDefinition typeDef) => typeDef == null ? " Null TypeDef" : $" TypeDef [{typeDef.FullName}]";
        public string TypeReferenceTraceText(TypeReference typeRef) => typeRef == null ? " Null TypeRef" : $" TypeRef [{typeRef.FullName}]";
        public string FieldDefinitionTraceText(FieldDefinition fieldDef) => fieldDef == null ? " Null FieldRef" : $" TypeRef [{fieldDef.DeclaringType.FullName}] FieldDef [{fieldDef.Name}]";
        public string PropertyDefinitionTraceText(PropertyDefinition propertyDef) => propertyDef == null ? " Null PropertydRef" : $" TypeRef [{propertyDef.DeclaringType.FullName}] PropertyDef [{propertyDef.Name}]";
        
        /// <summary>
        /// Logs a warning.
        /// </summary>
        /// <param name = "msg"></param>
        internal void LogWarning(string msg)
        {
            Diagnostics.AddWarning(msg);
        }

        /// <summary>
        /// Logs an error.
        /// </summary>
        /// <param name = "msg"></param>
        internal void LogError(string msg)
        {
            Diagnostics.AddError(msg);
        }
        #endregion

        #region ImportReference.
        public MethodReference ImportReference(SR.MethodBase method)
        {
            return Module.ImportReference(method);
        }

        public MethodReference ImportReference(SR.MethodBase method, IGenericParameterProvider context)
        {
            return Module.ImportReference(method, context);
        }

        public TypeReference ImportReference(TypeReference type)
        {
            return Module.ImportReference(type);
        }

        public TypeReference ImportReference(TypeReference type, IGenericParameterProvider context)
        {
            return Module.ImportReference(type, context);
        }

        public FieldReference ImportReference(FieldReference field)
        {
            return Module.ImportReference(field);
        }

        public FieldReference ImportReference(FieldReference field, IGenericParameterProvider context)
        {
            return Module.ImportReference(field, context);
        }

        public MethodReference ImportReference(MethodReference method)
        {
            return Module.ImportReference(method);
        }

        public MethodReference ImportReference(MethodReference method, IGenericParameterProvider context)
        {
            return Module.ImportReference(method, context);
        }

        public TypeReference ImportReference(System.Type type)
        {
            return ImportReference(type, null);
        }

        public TypeReference ImportReference(System.Type type, IGenericParameterProvider context)
        {
            return Module.ImportReference(type, context);
        }

        public FieldReference ImportReference(SR.FieldInfo field)
        {
            return Module.ImportReference(field);
        }

        public FieldReference ImportReference(SR.FieldInfo field, IGenericParameterProvider context)
        {
            return Module.ImportReference(field, context);
        }
        #endregion
    }
}