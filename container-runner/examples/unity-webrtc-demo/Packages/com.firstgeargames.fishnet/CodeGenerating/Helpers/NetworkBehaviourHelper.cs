using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.CodeGenerating.Processing;
using FishNet.Configuring;
using FishNet.Managing.Logging;
using FishNet.Object;
using FishNet.Object.Delegating;
using FishNet.Object.Helping;
using FishNet.Object.Prediction.Delegating;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using MethodAttributes = MonoFN.Cecil.MethodAttributes;

namespace FishNet.CodeGenerating.Helping
{
    internal class NetworkBehaviourHelper : CodegenBase
    {
        #region Reflection references.
        // Names.
        internal string FullName;
        // Prediction.
        public MethodReference Replicate_Reader_MethodRef;
        public MethodReference Reconcile_Server_MethodRef;
        // public FieldReference UsesPrediction_FieldRef;
        public MethodReference EmptyReplicatesQueueIntoHistory_Start_MethodRef;
        public MethodReference EmptyReplicatesQueueIntoHistory_MethodRef;
        public MethodReference Reconcile_Client_Start_MethodRef;
        public MethodReference Replicate_Replay_Start_MethodRef;
        public MethodReference Replicate_Current_MethodRef;
        public MethodReference Reconcile_Client_MethodRef;
        public MethodReference Reconcile_Current_MethodRef;
        public MethodReference ClearReplicateCache_Internal_MethodRef;
        public MethodReference Replicate_Replay_MethodRef;
        public MethodReference Reconcile_Reader_MethodRef;
        public MethodReference RegisterReplicateRpc_MethodRef;
        public MethodReference RegisterReconcileRpc_MethodRef;
        public MethodReference ReplicateRpcDelegate_Ctor_MethodRef;
        public MethodReference ReconcileRpcDelegate_Ctor_MethodRef;
        // public MethodReference Replicate_Server_SendToSpectators_MethodRef;
        // RPCs.
        public MethodReference SendServerRpc_MethodRef;
        public MethodReference SendObserversRpc_MethodRef;
        public MethodReference SendTargetRpc_MethodRef;
        public MethodReference RegisterServerRpc_MethodRef;
        public MethodReference RegisterObserversRpc_MethodRef;
        public MethodReference RegisterTargetRpc_MethodRef;
        public MethodReference ServerRpcDelegate_Ctor_MethodRef;
        public MethodReference ClientRpcDelegate_Ctor_MethodRef;
        // Is checks.
        public MethodReference IsClientInitialized_MethodRef;
        public MethodReference IsOwner_MethodRef;
        public MethodReference IsServerInitialized_MethodRef;
        public MethodReference IsHost_MethodRef;
        public MethodReference IsNetworked_MethodRef;
        // Misc.
        public TypeReference TypeRef;
        public MethodReference OwnerMatches_MethodRef;
        public MethodReference LocalConnection_MethodRef;
        public MethodReference Owner_MethodRef;
        public MethodReference NetworkInitializeIfDisabled_MethodRef;
        // TimeManager.
        public MethodReference TimeManager_MethodRef;
        #endregion

        #region Const.
        internal const uint MAX_SYNCTYPE_ALLOWANCE = byte.MaxValue;
        internal const uint MAX_RPC_ALLOWANCE = ushort.MaxValue;
        internal const uint MAX_PREDICTION_ALLOWANCE = byte.MaxValue;
        internal const string AWAKE_METHOD_NAME = "Awake";
        internal const string DISABLE_LOGGING_TEXT = "This message may be disabled by setting the Logging field in your attribute to LoggingType.Off";
        #endregion

        public override bool ImportReferences()
        {
            Type networkBehaviourType = typeof(NetworkBehaviour);
            TypeRef = ImportReference(networkBehaviourType);
            FullName = networkBehaviourType.FullName;
            ImportReference(networkBehaviourType);

            // ServerRpcDelegate and ClientRpcDelegate constructors.
            ServerRpcDelegate_Ctor_MethodRef = ImportReference(typeof(ServerRpcDelegate).GetConstructors().First());
            ClientRpcDelegate_Ctor_MethodRef = ImportReference(typeof(ClientRpcDelegate).GetConstructors().First());
            // Prediction Rpc delegate constructors.
            ReplicateRpcDelegate_Ctor_MethodRef = ImportReference(typeof(ReplicateRpcDelegate).GetConstructors().First());
            ReconcileRpcDelegate_Ctor_MethodRef = ImportReference(typeof(ReconcileRpcDelegate).GetConstructors().First());

            foreach (MethodInfo mi in networkBehaviourType.GetMethods(BindingFlags.Static | BindingFlags.Public | BindingFlags.Instance | BindingFlags.NonPublic))
            {
                if (mi.Name == nameof(NetworkBehaviour.GetIsNetworked))
                    IsNetworked_MethodRef = ImportReference(mi);
                // CreateDelegates.
                else if (mi.Name == nameof(NetworkBehaviour.RegisterServerRpc))
                    RegisterServerRpc_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.RegisterObserversRpc))
                    RegisterObserversRpc_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.RegisterTargetRpc))
                    RegisterTargetRpc_MethodRef = ImportReference(mi);
                // Prediction delegates.
                else if (mi.Name == nameof(NetworkBehaviour.RegisterReplicateRpc))
                    RegisterReplicateRpc_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.RegisterReconcileRpc))
                    RegisterReconcileRpc_MethodRef = ImportReference(mi);
                // SendRpcs.
                else if (mi.Name == nameof(NetworkBehaviour.SendServerRpc))
                    SendServerRpc_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.SendObserversRpc))
                    SendObserversRpc_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.SendTargetRpc))
                    SendTargetRpc_MethodRef = ImportReference(mi);
                // Misc.
                else if (mi.Name == nameof(NetworkBehaviour.OwnerMatches))
                    OwnerMatches_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.NetworkInitializeIfDisabled))
                    NetworkInitializeIfDisabled_MethodRef = ImportReference(mi);
                // Prediction
                else if (mi.Name == nameof(NetworkBehaviour.Replicate_Current))
                    Replicate_Current_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Replicate_Replay_Start))
                    Replicate_Replay_Start_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.EmptyReplicatesQueueIntoHistory))
                    EmptyReplicatesQueueIntoHistory_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.EmptyReplicatesQueueIntoHistory_Start))
                    EmptyReplicatesQueueIntoHistory_Start_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Reconcile_Client_Start))
                    Reconcile_Client_Start_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Replicate_Replay))
                    Replicate_Replay_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Replicate_Reader))
                    Replicate_Reader_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Reconcile_Reader_Remote))
                    Reconcile_Reader_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Reconcile_Server))
                    Reconcile_Server_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Reconcile_Client))
                    Reconcile_Client_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.Reconcile_Current))
                    Reconcile_Current_MethodRef = ImportReference(mi);
                else if (mi.Name == nameof(NetworkBehaviour.ClearReplicateCache_Internal))
                    ClearReplicateCache_Internal_MethodRef = ImportReference(mi);
            }

            foreach (PropertyInfo pi in networkBehaviourType.GetProperties(BindingFlags.Static | BindingFlags.Public | BindingFlags.Instance | BindingFlags.NonPublic))
            {
                // Server/Client states.
                if (pi.Name == nameof(NetworkBehaviour.IsClientInitialized))
                    IsClientInitialized_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(NetworkBehaviour.IsServerInitialized))
                    IsServerInitialized_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(NetworkBehaviour.IsHostStarted))
                    IsHost_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(NetworkBehaviour.IsOwner))
                    IsOwner_MethodRef = ImportReference(pi.GetMethod);
                // Owner.
                else if (pi.Name == nameof(NetworkBehaviour.Owner))
                    Owner_MethodRef = ImportReference(pi.GetMethod);
                else if (pi.Name == nameof(NetworkBehaviour.LocalConnection))
                    LocalConnection_MethodRef = ImportReference(pi.GetMethod);
                // Misc.
                else if (pi.Name == nameof(NetworkBehaviour.TimeManager))
                    TimeManager_MethodRef = ImportReference(pi.GetMethod);
            }

            return true;
        }

        /// <summary>
        /// Returnsthe child most Awake by iterating up childMostTypeDef.
        /// </summary>
        /// <param name = "childMostTypeDef"></param>
        /// <param name = "created"></param>
        /// <returns></returns>
        internal MethodDefinition GetAwakeMethodDefinition(TypeDefinition typeDef)
        {
            return typeDef.GetMethod(AWAKE_METHOD_NAME);
        }

        /// <summary>
        /// Creates a replicate delegate.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "originalMethodDef"></param>
        /// <param name = "readerMethodDef"></param>
        /// <param name = "rpcType"></param>
        internal void CreateReplicateDelegate(MethodDefinition originalMethodDef, MethodDefinition readerMethodDef, uint methodHash)
        {
            MethodDefinition methodDef = originalMethodDef.DeclaringType.GetMethod(NetworkBehaviourProcessor.NETWORKINITIALIZE_EARLY_INTERNAL_NAME);
            ILProcessor processor = methodDef.Body.GetILProcessor();

            List<Instruction> insts = new();
            insts.Add(processor.Create(OpCodes.Ldarg_0));

            insts.Add(processor.Create(OpCodes.Ldc_I4, (int)methodHash));

            /* Create delegate and call NetworkBehaviour method. */
            insts.Add(processor.Create(OpCodes.Ldnull));
            insts.Add(processor.Create(OpCodes.Ldftn, readerMethodDef));

            /* Has to be done last. This allows the NetworkBehaviour to
             * initialize it's fields first. */
            processor.InsertLast(insts);
        }

        /// <summary>
        /// Creates a RPC delegate for rpcType.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "originalMethodDef"></param>
        /// <param name = "readerMethodDef"></param>
        /// <param name = "rpcType"></param>
        internal void CreateRpcDelegate(bool runLocally, TypeDefinition typeDef, MethodDefinition readerMethodDef, RpcType rpcType, uint methodHash, CustomAttribute rpcAttribute)
        {

            MethodDefinition methodDef = typeDef.GetMethod(NetworkBehaviourProcessor.NETWORKINITIALIZE_EARLY_INTERNAL_NAME);
            ILProcessor processor = methodDef.Body.GetILProcessor();

            List<Instruction> insts = new();
            insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Ldc_I4, (int)methodHash));

            /* Create delegate and call NetworkBehaviour method. */
            insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Ldftn, readerMethodDef));
            // Server.
            if (rpcType == RpcType.Server)
            {
                insts.Add(processor.Create(OpCodes.Newobj, ServerRpcDelegate_Ctor_MethodRef));
                insts.Add(processor.Create(OpCodes.Call, RegisterServerRpc_MethodRef));
            }
            // Observers.
            else if (rpcType == RpcType.Observers)
            {
                insts.Add(processor.Create(OpCodes.Newobj, ClientRpcDelegate_Ctor_MethodRef));
                insts.Add(processor.Create(OpCodes.Call, RegisterObserversRpc_MethodRef));
            }
            // Target
            else if (rpcType == RpcType.Target)
            {
                insts.Add(processor.Create(OpCodes.Newobj, ClientRpcDelegate_Ctor_MethodRef));
                insts.Add(processor.Create(OpCodes.Call, RegisterTargetRpc_MethodRef));
            }

            /* Has to be done last. This allows the NetworkBehaviour to
             * initialize it's fields first. */
            processor.InsertLast(insts);
        }

        /// <summary>
        /// Creates exit method condition if local client is not owner.
        /// </summary>
        /// <param name = "retIfOwner">True if to ret when owner, false to ret when not owner.</param>
        /// <returns>Returns Ret instruction.</returns>
        internal Instruction CreateLocalClientIsOwnerCheck(MethodDefinition methodDef, LoggingType loggingType, bool notifyMessageCanBeDisabled, bool retIfOwner, bool insertFirst)
        {
            List<Instruction> instructions = new();
            /* This is placed after the if check.
             * Should the if check pass then code
             * jumps to this instruction. */
            ILProcessor processor = methodDef.Body.GetILProcessor();
            Instruction endIf = processor.Create(OpCodes.Nop);

            instructions.Add(processor.Create(OpCodes.Ldarg_0)); // argument: this
            // If !base.IsOwner endIf.
            instructions.Add(processor.Create(OpCodes.Call, IsOwner_MethodRef));
            if (retIfOwner)
                instructions.Add(processor.Create(OpCodes.Brfalse, endIf));
            else
                instructions.Add(processor.Create(OpCodes.Brtrue, endIf));
            // If logging is not disabled.
            if (loggingType != LoggingType.Off)
            {
                string disableLoggingText = notifyMessageCanBeDisabled ? DISABLE_LOGGING_TEXT : string.Empty;
                string msg = retIfOwner ? $"Cannot complete action because you are the owner of this object. {disableLoggingText}." : $"Cannot complete action because you are not the owner of this object. {disableLoggingText}.";

                instructions.AddRange(GetClass<GeneralHelper>().LogMessage(methodDef, msg, loggingType));
            }
            // Return block.
            Instruction retInst = processor.Create(OpCodes.Ret);
            instructions.Add(retInst);
            // After if statement, jumped to when successful check.
            instructions.Add(endIf);

            if (insertFirst)
            {
                processor.InsertFirst(instructions);
            }
            else
            {
                foreach (Instruction inst in instructions)
                    processor.Append(inst);
            }

            return retInst;
        }

        /// <summary>
        /// Creates exit method condition if remote client is not owner.
        /// </summary>
        /// <param name = "processor"></param>
        internal Instruction CreateRemoteClientIsOwnerCheck(ILProcessor processor, ParameterDefinition connectionParameterDef)
        {
            /* This is placed after the if check.
             * Should the if check pass then code
             * jumps to this instruction. */
            Instruction endIf = processor.Create(OpCodes.Nop);

            processor.Emit(OpCodes.Ldarg_0); // argument: this
            // If !base.IsOwner endIf.
            processor.Emit(OpCodes.Ldarg, connectionParameterDef);
            processor.Emit(OpCodes.Call, OwnerMatches_MethodRef);
            processor.Emit(OpCodes.Brtrue, endIf);
            // Return block.
            Instruction retInst = processor.Create(OpCodes.Ret);
            processor.Append(retInst);

            // After if statement, jumped to when successful check.
            processor.Append(endIf);

            return retInst;
        }

        /// <summary>
        /// Creates exit method condition if not client.
        /// </summary>
        /// <param name = "useStatic">When true InstanceFinder.IsClient is used, when false base.IsClientInitialized is used.</param>
        internal void CreateIsClientCheck(MethodDefinition methodDef, LoggingType loggingType, bool useStatic, bool insertFirst, bool checkIsNetworked)
        {
            /* This is placed after the if check.
             * Should the if check pass then code
             * jumps to this instruction. */
            ILProcessor processor = methodDef.Body.GetILProcessor();
            Instruction conditionFailedInst = processor.Create(OpCodes.Nop);
            Instruction conditionPassedInst = processor.Create(OpCodes.Nop);

            List<Instruction> instructions = new();

            if (checkIsNetworked)
                instructions.AddRange(CreateIsNetworkedCheck(methodDef, OpCodes.Brtrue, conditionPassedInst));

            // Checking against the NetworkObject.
            if (!useStatic)
            {
                instructions.Add(processor.Create(OpCodes.Ldarg_0)); // argument: this
                // If (!base.IsClient)
                instructions.Add(processor.Create(OpCodes.Call, IsClientInitialized_MethodRef));
            }
            // Checking instanceFinder.
            else
            {
                instructions.Add(processor.Create(OpCodes.Call, GetClass<ObjectHelper>().InstanceFinder_IsClient_MethodRef));
            }
            
            instructions.Add(processor.Create(OpCodes.Brtrue, conditionPassedInst));
            instructions.Add(conditionFailedInst);
            
            // If warning then also append warning text.
            if (loggingType != LoggingType.Off)
            {
                string msg = $"Cannot complete action because client is not active. This may also occur if the object is not yet initialized, has deinitialized, or if it does not contain a NetworkObject component.";
                instructions.AddRange(GetClass<GeneralHelper>().LogMessage(methodDef, msg, loggingType));
            }
            // Add return.
            instructions.AddRange(CreateRetDefault(methodDef));
            // After if statement, jumped to when successful check.
            instructions.Add(conditionPassedInst);

            if (insertFirst)
            {
                processor.InsertFirst(instructions);
            }
            else
            {
                foreach (Instruction inst in instructions)
                    processor.Append(inst);
            }
        }

        /// <summary>
        /// Creates exit method condition if not server.
        /// </summary>
        /// <param name = "useStatic">When true InstanceFinder.IsServer is used, when false base.IsServerInitialized is used.</param>
        internal void CreateIsServerCheck(MethodDefinition methodDef, LoggingType loggingType, bool useStatic, bool insertFirst, bool checkIsNetworked)
        {
            /* This is placed after the if check.
             * Should the if check pass then code
             * jumps to this instruction. */
            ILProcessor processor = methodDef.Body.GetILProcessor();
            Instruction conditionFailedInst = processor.Create(OpCodes.Nop);
            Instruction conditionPassedInst = processor.Create(OpCodes.Nop);

            List<Instruction> instructions = new();

            if (checkIsNetworked)
                instructions.AddRange(CreateIsNetworkedCheck(methodDef, OpCodes.Brfalse, conditionFailedInst));

            if (!useStatic)
            {
                instructions.Add(processor.Create(OpCodes.Ldarg_0)); // argument: this
                // If (!base.IsServer)
                instructions.Add(processor.Create(OpCodes.Call, IsServerInitialized_MethodRef));
            }
            // Checking instanceFinder.
            else
            {
                instructions.Add(processor.Create(OpCodes.Call, GetClass<ObjectHelper>().InstanceFinder_IsServer_MethodRef));
            }

            instructions.Add(processor.Create(OpCodes.Brtrue, conditionPassedInst));
            instructions.Add(conditionFailedInst);

            // If warning then also append warning text.
            if (loggingType != LoggingType.Off)
            {
                string msg = $"Cannot complete action because server is not active. This may also occur if the object is not yet initialized, has deinitialized, or if it does not contain a NetworkObject component.";
                instructions.AddRange(GetClass<GeneralHelper>().LogMessage(methodDef, msg, loggingType));
            }
            // Add return.
            instructions.AddRange(CreateRetDefault(methodDef));
            // After if statement, jumped to when successful check.
            instructions.Add(conditionPassedInst);

            if (insertFirst)
            {
                processor.InsertFirst(instructions);
            }
            else
            {
                foreach (Instruction inst in instructions)
                    processor.Append(inst);
            }
        }

        /// <summary>
        /// Creates a call to base.IsNetworked and returns instructions.
        /// </summary>
        private List<Instruction> CreateIsNetworkedCheck(MethodDefinition methodDef, OpCode conditionalOpCode, Instruction endIfInst)
        {
            if (conditionalOpCode != OpCodes.Brfalse && conditionalOpCode != OpCodes.Brtrue && conditionalOpCode != OpCodes.Brfalse_S && conditionalOpCode != OpCodes.Brtrue_S)
            {
                Session.LogError($"OpCode {conditionalOpCode} is not supported for method {nameof(CreateIsNetworkedCheck)}.");
                return new();
            }

            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();
            insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Call, GetClass<NetworkBehaviourHelper>().IsNetworked_MethodRef));
            insts.Add(processor.Create(conditionalOpCode, endIfInst));

            return insts;
        }

        /// <summary>
        /// Creates a return using the ReturnType for methodDef.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "methodDef"></param>
        /// <returns></returns>
        public List<Instruction> CreateRetDefault(MethodDefinition methodDef, ModuleDefinition importReturnModule = null)
        {
            ILProcessor processor = methodDef.Body.GetILProcessor();
            List<Instruction> instructions = new();
            // If requires a value return.
            if (methodDef.ReturnType != methodDef.Module.TypeSystem.Void)
            {
                // Import type first.
                methodDef.Module.ImportReference(methodDef.ReturnType);
                if (importReturnModule != null)
                    importReturnModule.ImportReference(methodDef.ReturnType);
                VariableDefinition vd = GetClass<GeneralHelper>().CreateVariable(methodDef, methodDef.ReturnType);
                instructions.Add(processor.Create(OpCodes.Ldloca_S, vd));
                instructions.Add(processor.Create(OpCodes.Initobj, vd.VariableType));
                instructions.Add(processor.Create(OpCodes.Ldloc, vd));
            }
            instructions.Add(processor.Create(OpCodes.Ret));

            return instructions;
        }
    }
}