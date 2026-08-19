using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Connection;
using FishNet.Object;
using FishNet.Object.Synchronizing;
using FishNet.Object.Synchronizing.Internal;
using MonoFN.Cecil;
using System;
using System.Collections.Generic;
using System.Reflection;

namespace FishNet.CodeGenerating.Helping
{
    internal class ObjectHelper : CodegenBase
    {
        #region Reflection references.
        // Fullnames.
        public string SyncVar_Name { get; private set; }
        public string SyncList_Name { get; private set; }
        public string SyncDictionary_Name { get; private set; }
        public string SyncHashSet_Name { get; private set; }
        // Is checks.
        public MethodReference InstanceFinder_IsServer_MethodRef { get; private set; }
        public MethodReference InstanceFinder_IsClient_MethodRef { get; private set; }
        // Misc.
        public TypeReference NetworkBehaviour_TypeRef { get; private set; }
        public MethodReference NetworkConnection_IsValid_MethodRef { get; private set; }
        public MethodReference NetworkConnection_IsActive_MethodRef { get; private set; }
        public MethodReference Dictionary_Add_UShort_SyncBase_MethodRef { get; private set; }
        public MethodReference NetworkConnection_GetIsLocalClient_MethodRef { get; private set; }
        #endregion

        public override bool ImportReferences()
        {
            Type tmpType;
            /* SyncType names */
            // SyncVar.
            tmpType = typeof(SyncVar<>);
            ImportReference(tmpType);
            SyncVar_Name = tmpType.Name;
            // SyncList.
            tmpType = typeof(SyncList<>);
            ImportReference(tmpType);
            SyncList_Name = tmpType.Name;
            // SyncDictionary.
            tmpType = typeof(SyncDictionary<,>);
            ImportReference(tmpType);
            SyncDictionary_Name = tmpType.Name;
            // SyncHashSet.
            tmpType = typeof(SyncHashSet<>);
            ImportReference(tmpType);
            SyncHashSet_Name = tmpType.Name;

            NetworkBehaviour_TypeRef = ImportReference(typeof(NetworkBehaviour));

            tmpType = typeof(NetworkConnection);
            TypeReference networkConnectionTr = ImportReference(tmpType);
            foreach (PropertyDefinition item in networkConnectionTr.CachedResolve(Session).Properties)
            {
                if (item.Name == nameof(NetworkConnection.IsLocalClient))
                    NetworkConnection_GetIsLocalClient_MethodRef = ImportReference(item.GetMethod);
            }

            // Dictionary.Add(ushort, SyncBase).
            Type dictType = typeof(Dictionary<ushort, SyncBase>);
            TypeReference dictTypeRef = ImportReference(dictType);
            // Dictionary_Add_UShort_SyncBase_MethodRef = dictTypeRef.CachedResolve(base.Session).GetMethod("add_Item", )
            foreach (MethodDefinition item in dictTypeRef.CachedResolve(Session).Methods)
            {
                if (item.Name == nameof(Dictionary<ushort, SyncBase>.Add))
                {
                    Dictionary_Add_UShort_SyncBase_MethodRef = ImportReference(item);
                    break;
                }
            }

            // InstanceFinder infos.
            Type instanceFinderType = typeof(InstanceFinder);
            foreach (PropertyInfo pi in instanceFinderType.GetProperties())
            {
                if (pi.Name == nameof(InstanceFinder.IsClientStarted))
                    InstanceFinder_IsClient_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(InstanceFinder.IsServerStarted))
                    InstanceFinder_IsServer_MethodRef = ImportReference(pi.GetMethod);
            }

            // NetworkConnection.
            foreach (PropertyInfo pi in typeof(NetworkConnection).GetProperties(BindingFlags.Static | BindingFlags.Public | BindingFlags.Instance | BindingFlags.NonPublic))
            {
                if (pi.Name == nameof(NetworkConnection.IsValid))
                    NetworkConnection_IsValid_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(NetworkConnection.IsActive))
                    NetworkConnection_IsActive_MethodRef = ImportReference(pi.GetMethod);
            }

            return true;
        }
    }
}