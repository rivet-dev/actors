// using FishNet.CodeGenerating.Helping.Extension;
// using FishNet.CodeGenerating.Processing;
// using FishNet.Object.Synchronizing;
// using FishNet.Object.Synchronizing.Internal;
// using MonoFN.Cecil;
// using MonoFN.Cecil.Rocks;
// using System;
// using System.Collections.Generic;

// namespace FishNet.CodeGenerating.Helping
// {
//    internal class CreatedSyncVarGenerator : CodegenBase
//    {
//        private readonly Dictionary<string, DeclaredSyncType> _createdSyncVars = new Dictionary<string, DeclaredSyncType>();

//        #region Relfection references.
//        private TypeReference _syncBase_TypeRef;
//        internal TypeReference SyncVar_TypeRef;
//        private MethodReference _syncVar_InitializeOnce_MethodRef;
//        #endregion

//        #region Const.
//        private const string GETVALUE_NAME = "GetValue";
//        private const string SETVALUE_NAME = "SetValue";
//        #endregion

//        /* // feature add and test the dirty boolean changes
//         * eg... instead of base.Dirty()
//         * do if (!base.Dirty()) return false;
//         * See synclist for more info. */

//        /// <summary>
//        /// Imports references needed by this helper.
//        /// </summary>
//        /// <param name="moduleDef"></param>
//        /// <returns></returns>
//        public override bool ImportReferences()
//        {
//            Type svType = typeof(SyncVar<>);
//            SyncVar_TypeRef = base.ImportReference(svType);
//            _syncVar_InitializeOnce_MethodRef = base.ImportReference(svType.GetMethod(nameof(SyncVar<int>.InitializeOnce)));
//            Type syncBaseType = typeof(SyncBase);
//            _syncBase_TypeRef = base.ImportReference(syncBaseType);

//            return true;
//        }

//        /// <summary>
//        /// Gets and optionally creates data for SyncVar<typeOfField>
//        /// </summary>
//        /// <param name="dataTr"></param>
//        /// <returns></returns>
//        internal DeclaredSyncType GetCreatedSyncVar(FieldDefinition originalFd, bool createMissing)
//        {
//            TypeReference dataTr = originalFd.FieldType;
//            TypeDefinition dataTd = dataTr.CachedResolve(base.Session);

//            string typeHash = dataTr.FullName + dataTr.IsArray.ToString();

//            if (_createdSyncVars.TryGetValue(typeHash, out DeclaredSyncType createdSyncVar))
//            {
//                return createdSyncVar;
//            }
//            else
//            {
//                if (!createMissing)
//                    return null;

//                base.ImportReference(dataTd);

//                GenericInstanceType originalFdGit = SyncVar_TypeRef.MakeGenericInstanceType(new TypeReference[] { dataTr });
//                TypeReference genericDataTr = originalFdGit.GenericArguments[0];

//                // Make sure can serialize.
//                bool canSerialize = base.GetClass<GeneralHelper>().HasSerializerAndDeserializer(genericDataTr, true);
//                if (!canSerialize)
//                {
//                    base.LogError($"SyncVar {originalFd.Name} data type {genericDataTr.FullName} does not support serialization. Use a supported type or create a custom serializer.");
//                    return null;
//                }

//                // Set needed methods from syncbase.
//                MethodReference setSyncIndexMr;
//                MethodReference initializeOnceMrGit = _syncVar_InitializeOnce_MethodRef.MakeHostInstanceGeneric(base.Session, originalFdGit);

//                if (!base.GetClass<SyncTypeProcessor>().SetSyncBaseMethods(_syncBase_TypeRef.CachedResolve(base.Session), out setSyncIndexMr, out _))
//                    return null;

//                MethodReference setValueMr = null;
//                MethodReference getValueMr = null;
//                foreach (MethodDefinition md in SyncVar_TypeRef.CachedResolve(base.Session).Methods)
//                {
//                    // GetValue.
//                    if (md.Name == GETVALUE_NAME)
//                    {
//                        MethodReference mr = base.ImportReference(md);
//                        getValueMr = mr.MakeHostInstanceGeneric(base.Session, originalFdGit);
//                    }
//                    // SetValue.
//                    else if (md.Name == SETVALUE_NAME)
//                    {
//                        MethodReference mr = base.ImportReference(md);
//                        setValueMr = mr.MakeHostInstanceGeneric(base.Session, originalFdGit);
//                    }
//                }

//                if (setValueMr == null || getValueMr == null)
//                    return null;

//                DeclaredSyncType csv = new DeclaredSyncType(originalFd, originalFdGit, dataTd, initializeOnceMrGit, null);
//                _createdSyncVars.Add(typeHash, csv);
//                return csv;
//            }
//        }

//    }

// }

