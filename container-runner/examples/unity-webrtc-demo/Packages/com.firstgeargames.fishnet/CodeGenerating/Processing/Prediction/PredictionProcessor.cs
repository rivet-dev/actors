using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.CodeGenerating.Processing.Rpc;
using FishNet.Connection;
using FishNet.Object;
using FishNet.Object.Prediction;
using FishNet.Object.Prediction.Delegating;
using FishNet.Serializing;
using FishNet.Transporting;
using FishNet.Utility.Performance;
using GameKit.Dependencies.Utilities;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using MonoFN.Cecil.Rocks;
using System.Collections.Generic;
using System.Linq;
using GameKit.Dependencies.Utilities.Types;
using UnityEngine;
using SR = System.Reflection;

namespace FishNet.CodeGenerating.Processing
{
    internal class PredictionProcessor : CodegenBase
    {
        #region Types.
        private class PredictionAttributedMethods
        {
            public MethodDefinition ReplicateMethod;
            public MethodDefinition ReconcileMethod;

            public PredictionAttributedMethods(MethodDefinition replicateMethod, MethodDefinition reconcileMethod)
            {
                ReplicateMethod = replicateMethod;
                ReconcileMethod = reconcileMethod;
            }
        }

        private enum InsertType
        {
            First,
            Last,
            Current
        }

        private class CreatedPredictionFields
        {
            /// <summary>
            /// TypeReference of replicate data.
            /// </summary>
            public TypeReference ReplicateDataTypeRef;
            /// <summary>
            /// Typereference of reconcile data.
            /// </summary>
            public TypeReference ReconcileDataTypeRef;
            /// <summary>
            /// Delegate for calling replicate user logic.
            /// </summary>
            public FieldDefinition ReplicateUserLogicDelegate;
            /// <summary>
            /// Delegate for calling replicate user logic.
            /// </summary>
            public FieldDefinition ReconcileUserLogicDelegate;
            /// <summary>
            /// Replicate data which has not run yet and is in queue to do so.
            /// </summary>
            public FieldDefinition ReplicatesQueue;
            /// <summary>
            /// Replicate data which has already run and is used to reconcile/replay.
            /// </summary>
            public FieldDefinition ReplicatesHistory;
            /// <summary>
            /// Reconcile data cached locally from the local client.
            /// </summary>
            public FieldDefinition LocalReconciles;
            /// <summary>
            /// Last replicate read. This is used for reading delta replicates.
            /// </summary>
            public FieldDefinition LastReadReplicate;
            /// <summary>
            /// Last reconcile read. This is used for reading delta reconciles.
            /// </summary>
            public FieldDefinition LastReadReconcile;
        }

        private class PredictionReaders
        {
            public readonly MethodReference ReplicateReader;
            public readonly MethodReference ReconcileReader;

            public PredictionReaders(MethodReference replicateReader, MethodReference reconcileReader)
            {
                ReplicateReader = replicateReader;
                ReconcileReader = reconcileReader;
            }
        }
        #endregion

        #region Public.
        public string IReplicateData_FullName = typeof(IReplicateData).FullName;
        public string IReconcileData_FullName = typeof(IReconcileData).FullName;
        public TypeReference ReplicateULDelegate_TypeRef;
        public TypeReference ReconcileULDelegate_TypeRef;
        public MethodReference IReplicateData_GetTick_MethodRef;
        public MethodReference IReconcileData_GetTick_MethodRef;
        public MethodReference ReplicateData_Ctor_MethodRef;
        #endregion

        #region Const.
        public const string REPLICATE_READER_PREFIX = "Reader_Replicate___";
        public const string RECONCILE_READER_PREFIX = "Reader_Reconcile___";
        #endregion

        public override bool ImportReferences()
        {
            System.Type locType;
            // SR.MethodInfo locMi;

            ImportReference(typeof(BasicQueue<>));
            ReplicateULDelegate_TypeRef = ImportReference(typeof(ReplicateUserLogicDelegate<>));
            ReconcileULDelegate_TypeRef = ImportReference(typeof(ReconcileUserLogicDelegate<>));

            TypeDefinition replicateDataTd = ImportReference(typeof(ReplicateDataContainer<>)).CachedResolve(Session);
            ReplicateData_Ctor_MethodRef = ImportReference(replicateDataTd.GetConstructor(parameterCount: 2));

            // Get/Set tick.
            locType = typeof(IReplicateData);
            foreach (SR.MethodInfo mi in locType.GetMethods())
            {
                if (mi.Name == nameof(IReplicateData.GetTick))
                {
                    IReplicateData_GetTick_MethodRef = ImportReference(mi);
                    break;
                }
            }

            locType = typeof(IReconcileData);
            foreach (SR.MethodInfo mi in locType.GetMethods())
            {
                if (mi.Name == nameof(IReconcileData.GetTick))
                {
                    IReconcileData_GetTick_MethodRef = ImportReference(mi);
                    break;
                }
            }

            return true;
        }

        #region Setup and checks.
        /// <summary>
        /// Gets number of predictions by checking for prediction attributes. This does not perform error checking.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        internal uint GetPredictionCount(TypeDefinition typeDef)
        {
            /* Currently only one prediction method is allowed per typeDef.
             * Return 1 soon as a method is found. */
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                foreach (CustomAttribute customAttribute in methodDef.CustomAttributes)
                {
                    if (customAttribute.Is(GetClass<AttributeHelper>().ReplicateAttribute_FullName))
                        return 1;
                }
            }

            return 0;
        }

        /// <summary>
        /// Gets number of predictions by checking for prediction attributes in typeDef parents, excluding typerDef.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        internal uint GetPredictionCountInParents(TypeDefinition typeDef)
        {
            uint count = 0;
            do
            {
                typeDef = typeDef.GetNextBaseClassToProcess(Session);
                if (typeDef != null)
                    count += GetPredictionCount(typeDef);
            } while (typeDef != null);

            return count;
        }

        /// <summary>
        /// Ensures only one prediction and reconile method exist per typeDef, and outputs finding.
        /// </summary>
        /// <returns>True if there is only one set of prediction methods. False if none, or more than one set.</returns>
        internal bool GetPredictionMethods(TypeDefinition typeDef, out MethodDefinition replicateMd, out MethodDefinition reconcileMd)
        {
            replicateMd = null;
            reconcileMd = null;

            string replicateAttributeFullName = GetClass<AttributeHelper>().ReplicateAttribute_FullName;
            string reconcileAttributeFullName = GetClass<AttributeHelper>().ReconcileAttribute_FullName;

            bool error = false;
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                foreach (CustomAttribute customAttribute in methodDef.CustomAttributes)
                {
                    if (customAttribute.Is(replicateAttributeFullName))
                    {
                        if (!IsMethodPrivate(methodDef) || IsPredictionMethodAlreadyFound(replicateMd))
                            error = true;
                        else
                            replicateMd = methodDef;
                    }
                    else if (customAttribute.Is(reconcileAttributeFullName))
                    {
                        if (!IsMethodPrivate(methodDef) || IsPredictionMethodAlreadyFound(reconcileMd))
                        {
                            error = true;
                        }
                        else
                        {
                            reconcileMd = methodDef;
                            if (!CheckCreateReconcile(reconcileMd))
                                error = true;
                        }
                    }

                    if (error)
                        break;
                }

                if (error)
                    break;
            }

            // Checks to make sure the CreateReconcile method exist and calls reconcile.
            bool CheckCreateReconcile(MethodDefinition reconcileMd)
            {
                string crName = nameof(NetworkBehaviour.CreateReconcile);
                MethodDefinition createReconcileMd = reconcileMd.DeclaringType.GetMethod(crName);
                // Does not exist.
                if (createReconcileMd == null)
                {
                    LogError($"{reconcileMd.DeclaringType.Name} does not implement method {crName}. Override method {crName} and use it to create your reconcile data, and call your reconcile method {reconcileMd.Name}. Call ");
                    return false;
                }
                // Exists, check for call.
                else
                {
                    // Check for call instructions.
                    foreach (Instruction inst in createReconcileMd.Body.Instructions)
                    {
                        if (inst.OpCode == OpCodes.Call || inst.OpCode == OpCodes.Callvirt)
                        {
                            if (inst.Operand is MethodReference mr)
                            {
                                if (mr.Name == reconcileMd.Name)
                                    return true;
                            }
                        }
                    }

                    LogError($"{reconcileMd.DeclaringType.Name} implements {crName} but does not call reconcile method {reconcileMd.Name}. If you are calling CreateReconcile from another type please make a new method to call in {reconcileMd.DeclaringType.Name}, which in return calls CreateReconcile.");
                    // Fallthrough.
                    return false;
                }
            }

            /* Forcing a method to private is not necessarily needed
             * but it adds a safe-guard against users calling base.Reconcile/Replicate
             * from another replicate. Doing this would cause the replicate to run twice
             * for the same script hierarchy, which would create unpredictable behavior. */
            bool IsMethodPrivate(MethodDefinition md)
            {
                bool isPrivate = md.Attributes.HasFlag(MethodAttributes.Private);
                if (!isPrivate)
                    LogError($"Method {md.Name} within {typeDef.Name} is a prediction method and must be private.");
                return isPrivate;
            }

            bool IsPredictionMethodAlreadyFound(MethodDefinition md)
            {
                bool alreadyFound = md != null;
                if (alreadyFound)
                    LogError($"{typeDef.Name} contains multiple prediction sets; currently only one set is allowed.");

                return alreadyFound;
            }

            if (!error && replicateMd == null != (reconcileMd == null))
            {
                LogError($"{typeDef.Name} must contain both a [Replicate] and [Reconcile] method when using prediction.");
                error = true;
            }

            if (error || replicateMd == null || reconcileMd == null)
                return false;
            else
                return true;
        }
        #endregion

        internal bool Process(TypeDefinition typeDef)
        {
            // Set prediction count in parents here. Increase count after each predictionAttributeMethods iteration.
            // Do a for each loop on predictionAttributedMethods.
            /* NOTES: get all prediction attributed methods up front and store them inside predictionAttributedMethods.
             * To find the proper reconciles for replicates add an attribute field allowing users to assign Ids. EG ReplicateV2.Id = 1. Default
             * value will be 0. */

            MethodDefinition replicateMd;
            MethodDefinition reconcileMd;
            //Not using prediction methods.
            if (!GetPredictionMethods(typeDef, out replicateMd, out reconcileMd))
                return false;

            RpcProcessor rp = GetClass<RpcProcessor>();
            uint predictionRpcCount = GetPredictionCountInParents(typeDef) + rp.GetRpcCountInParents(typeDef);

            //If replication methods found but this hierarchy already has max.
            if (predictionRpcCount >= NetworkBehaviourHelper.MAX_RPC_ALLOWANCE)
            {
                LogError($"{typeDef.FullName} and inherited types exceed {NetworkBehaviourHelper.MAX_PREDICTION_ALLOWANCE} replicated methods. Only {NetworkBehaviourHelper.MAX_PREDICTION_ALLOWANCE} replicated methods are supported per inheritance hierarchy.");
                return false;
            }

            bool parameterError = false;
            parameterError |= HasParameterError(replicateMd, typeDef, true);
            parameterError |= HasParameterError(reconcileMd, typeDef, false);
            if (parameterError)
                return false;

            TypeDefinition replicateDataTd = replicateMd.Parameters[0].ParameterType.CachedResolve(Session);
            TypeDefinition reconcileDataTd = reconcileMd.Parameters[0].ParameterType.CachedResolve(Session);
            //Ensure datas implement interfaces.
            bool interfacesImplemented = true;
            DataImplementInterfaces(replicateMd, true, ref interfacesImplemented);
            DataImplementInterfaces(reconcileMd, false, ref interfacesImplemented);
            if (!interfacesImplemented)
                return false;
            if (!TickFieldIsNonSerializable(replicateDataTd, true))
                return false;
            if (!TickFieldIsNonSerializable(reconcileDataTd, false))
                return false;

            /* Make sure data can serialize. Use array type, this will
             * generate a serializer for element type as well. */
            bool canSerialize;
            //Make sure replicate data can serialize.
            canSerialize = GetClass<GeneralHelper>().HasSerializerAndDeserializer(replicateDataTd.MakeArrayType(), true);
            if (!canSerialize)
            {
                LogError($"Replicate data type {replicateDataTd.Name} does not support serialization. Use a supported type or create a custom serializer.");
                return false;
            }

            //Make sure reconcile data can serialize.
            canSerialize = GetClass<GeneralHelper>().HasSerializerAndDeserializer(reconcileDataTd, true);
            if (!canSerialize)
            {
                LogError($"Reconcile data type {reconcileDataTd.Name} does not support serialization. Use a supported type or create a custom serializer.");
                return false;
            }

            //Creates fields for buffers.
            CreatedPredictionFields predictionFields;
            CreateFields(typeDef, replicateMd, reconcileMd, out predictionFields);

            PredictionReaders predictionReaders;
            MethodDefinition replicateULMd;
            MethodDefinition reconcileULMd;
            CreatePredictionMethods(typeDef, replicateMd, reconcileMd, predictionFields, predictionRpcCount, out predictionReaders, out replicateULMd, out reconcileULMd);
            InitializeCollections(typeDef, replicateMd, reconcileMd, predictionFields);
            InitializeULDelegates(typeDef, predictionFields, replicateMd, reconcileMd, replicateULMd, reconcileULMd);
            RegisterPredictionRpcs(typeDef, predictionRpcCount, predictionReaders);

            return true;
        }

        /// <summary>
        /// Ensures the tick field for GetTick is non-serializable.
        /// </summary>
        private bool TickFieldIsNonSerializable(TypeDefinition dataTd, bool replicate)
        {
            string methodName = replicate ? IReplicateData_GetTick_MethodRef.Name : IReconcileData_GetTick_MethodRef.Name;
            MethodDefinition getMd = dataTd.GetMethodDefinitionInAnyBase(Session, methodName);

            //Try to find ldFld.
            Instruction ldFldInst = null;
            foreach (Instruction item in getMd.Body.Instructions)
            {
                if (item.OpCode == OpCodes.Ldfld)
                {
                    ldFldInst = item;
                    break;
                }
            }

            //If ldFld not found.
            if (ldFldInst == null)
            {
                LogError($"{dataTd.FullName} method {getMd.Name} does not return a field type for the Tick. Make a new private or protected field of uint type and return it's value within {getMd.Name}.");
                return false;
            }

            //Make sure the field is correct accessibility
            FieldDefinition fd = (FieldDefinition)ldFldInst.Operand;
            if (!fd.Attributes.HasFlag(FieldAttributes.Private) && !fd.Attributes.HasFlag(FieldAttributes.Family))
            {
                LogError($"{dataTd.FullName} method {getMd.Name} returns a tick field it does not have the correct accessibility. Make the field {fd.Name} private or protected.");
                return false;
            }

            //All checks pass.
            return true;
        }

        private void DataImplementInterfaces(MethodDefinition methodDef, bool isReplicate, ref bool interfacesImplemented)
        {
            TypeReference dataTr = methodDef.Parameters[0].ParameterType;
            string interfaceName = isReplicate ? IReplicateData_FullName : IReconcileData_FullName;
            //If does not implement.
            if (!dataTr.CachedResolve(Session).ImplementsInterfaceRecursive(Session, interfaceName))
            {
                string name = isReplicate ? typeof(IReplicateData).Name : typeof(IReconcileData).Name;
                LogError($"Prediction data type {dataTr.Name} for method {methodDef.Name} in class {methodDef.DeclaringType.Name} must implement the {name} interface.");
                interfacesImplemented = false;
            }
        }

        /// <summary>
        /// Registers RPCs that prediction uses.
        /// </summary>
        private void RegisterPredictionRpcs(TypeDefinition typeDef, uint hash, PredictionReaders readers)
        {
            MethodDefinition injectionMethodDef = typeDef.GetMethod(NetworkBehaviourProcessor.NETWORKINITIALIZE_EARLY_INTERNAL_NAME);
            ILProcessor processor = injectionMethodDef.Body.GetILProcessor();
            List<Instruction> insts = new();

            Register(readers.ReplicateReader.CachedResolve(Session), true);
            Register(readers.ReconcileReader.CachedResolve(Session), false);

            void Register(MethodDefinition readerMd, bool replicate)
            {
                insts.Add(processor.Create(OpCodes.Ldarg_0));
                insts.Add(processor.Create(OpCodes.Ldc_I4, (int)hash));
                /* Create delegate and call NetworkBehaviour method. */
                insts.Add(processor.Create(OpCodes.Ldarg_0));
                insts.Add(processor.Create(OpCodes.Ldftn, readerMd));

                MethodReference ctorMr;
                MethodReference callMr;
                if (replicate)
                {
                    ctorMr = GetClass<NetworkBehaviourHelper>().ReplicateRpcDelegate_Ctor_MethodRef;
                    callMr = GetClass<NetworkBehaviourHelper>().RegisterReplicateRpc_MethodRef;
                }
                else
                {
                    ctorMr = GetClass<NetworkBehaviourHelper>().ReconcileRpcDelegate_Ctor_MethodRef;
                    callMr = GetClass<NetworkBehaviourHelper>().RegisterReconcileRpc_MethodRef;
                }

                insts.Add(processor.Create(OpCodes.Newobj, ctorMr));
                insts.Add(processor.Create(OpCodes.Call, callMr));
            }

            processor.InsertLast(insts);
        }

        /// <summary>
        /// Initializes collection fields made during this process.
        /// </summary>
        /// <param name = "predictionFields"></param>
        private void InitializeCollections(TypeDefinition typeDef, MethodDefinition replicateMd, MethodDefinition reconcileMd, CreatedPredictionFields predictionFields)
        {
            GeneralHelper gh = GetClass<GeneralHelper>();
            TypeReference replicateDataTr = replicateMd.Parameters[0].ParameterType;
            TypeReference reconcileDataTr = reconcileMd.Parameters[0].ParameterType;
            MethodDefinition injectionMethodDef = typeDef.GetMethod(NetworkBehaviourProcessor.NETWORKINITIALIZE_EARLY_INTERNAL_NAME);
            ILProcessor processor = injectionMethodDef.Body.GetILProcessor();

            GenericInstanceType git;

            //ReplicateQueue.
            git = gh.GetGenericType(typeof(ReplicateDataContainer<>), replicateDataTr);
            Generate(predictionFields.ReplicatesQueue, gh.GetGenericBasicQueue(git), isRingBuffer: false);

            //ReplicatesHistory buffer.
            git = gh.GetGenericType(typeof(ReplicateDataContainer<>), replicateDataTr);
            Generate(predictionFields.ReplicatesHistory, gh.GetGenericRingBuffer(git), isRingBuffer: true);

            //LocalReconcile buffer.
            git = gh.GetGenericType(typeof(LocalReconcile<>), reconcileDataTr);
            Generate(predictionFields.LocalReconciles, gh.GetGenericRingBuffer(git), isRingBuffer: true);

            void Generate(FieldReference fr, GenericInstanceType lGit, bool isRingBuffer)
            {
                MethodDefinition ctorMd;
                if (isRingBuffer)
                    //ctorMd = base.GetClass<GeneralHelper>().RingBuffer_TypeRef.CachedResolve(base.Session).GetDefaultConstructor(base.Session);
                    ctorMd = GetClass<GeneralHelper>().RingBuffer_TypeRef.CachedResolve(Session).GetConstructor(Session, 1);
                else
                    ctorMd = GetClass<GeneralHelper>().List_TypeRef.CachedResolve(Session).GetDefaultConstructor(Session);

                MethodReference ctorMr = ctorMd.MakeHostInstanceGeneric(Session, lGit);

                List<Instruction> insts = new();

                insts.Add(processor.Create(OpCodes.Ldarg_0));
                if (isRingBuffer)
                    insts.Add(processor.Create(OpCodes.Ldc_I4, RingBuffer<int>.DEFAULT_CAPACITY));
                insts.Add(processor.Create(OpCodes.Newobj, ctorMr));
                insts.Add(processor.Create(OpCodes.Stfld, fr));

                processor.InsertFirst(insts);
            }
        }

        /// <summary>
        /// Initializes collection fields made during this process.
        /// </summary>
        /// <param name = "predictionFields"></param>
        private void InitializeULDelegates(TypeDefinition typeDef, CreatedPredictionFields predictionFields, MethodDefinition replicateMd, MethodDefinition reconcileMd, MethodDefinition replicateULMd, MethodDefinition reconcileULMd)
        {
            TypeReference replicateDataTr = replicateMd.Parameters[0].ParameterType;
            TypeReference reconcileDataTr = reconcileMd.Parameters[0].ParameterType;
            MethodDefinition injectionMethodDef = typeDef.GetMethod(NetworkBehaviourProcessor.NETWORKINITIALIZE_EARLY_INTERNAL_NAME);
            ILProcessor processor = injectionMethodDef.Body.GetILProcessor();
            List<Instruction> insts = new();

            Generate(replicateULMd, replicateDataTr, predictionFields.ReplicateUserLogicDelegate, typeof(ReplicateUserLogicDelegate<>), ReplicateULDelegate_TypeRef);
            Generate(reconcileULMd, reconcileDataTr, predictionFields.ReconcileUserLogicDelegate, typeof(ReconcileUserLogicDelegate<>), ReconcileULDelegate_TypeRef);

            void Generate(MethodDefinition ulMd, TypeReference dataTr, FieldReference fr, System.Type delegateType, TypeReference delegateTr)
            {
                insts.Clear();

                MethodDefinition ctorMd = delegateTr.CachedResolve(Session).GetFirstConstructor(Session, true);
                GenericInstanceType collectionGit;
                GetGenericULDelegate(dataTr, delegateType, out collectionGit);
                MethodReference ctorMr = ctorMd.MakeHostInstanceGeneric(Session, collectionGit);

                insts.Add(processor.Create(OpCodes.Ldarg_0));
                insts.Add(processor.Create(OpCodes.Ldarg_0));
                insts.Add(processor.Create(OpCodes.Ldftn, ulMd));
                insts.Add(processor.Create(OpCodes.Newobj, ctorMr));
                insts.Add(processor.Create(OpCodes.Stfld, fr));
                processor.InsertFirst(insts);
            }
        }

        /// <summary>
        /// Creates field buffers for replicate datas.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <param name = "replicateMd"></param>
        /// <param name = ""></param>
        /// <returns></returns>
        private void CreateFields(TypeDefinition typeDef, MethodDefinition replicateMd, MethodDefinition reconcileMd, out CreatedPredictionFields predictionFields)
        {
            GeneralHelper gh = GetClass<GeneralHelper>();
            TypeReference replicateDataTr = replicateMd.Parameters[0].ParameterType;
            TypeReference reconcileDataTr = reconcileMd.Parameters[0].ParameterType;

            GenericInstanceType git;

            //User logic delegate for replicates.
            GetGenericULDelegate(replicateDataTr, typeof(ReplicateUserLogicDelegate<>), out git);
            FieldDefinition replicateUserLogicDelegateFd = new($"_replicateULDelegate___{replicateMd.Name}", FieldAttributes.Private, git);
            typeDef.Fields.Add(replicateUserLogicDelegateFd);

            //User logic delegate for reconciles.
            GetGenericULDelegate(reconcileDataTr, typeof(ReconcileUserLogicDelegate<>), out git);
            FieldDefinition reconcileUserLogicDelegateFd = new($"_reconcileULDelegate___{reconcileMd.Name}", FieldAttributes.Private, git);
            typeDef.Fields.Add(reconcileUserLogicDelegateFd);

            //Replicates history.
            git = gh.GetGenericType(typeof(ReplicateDataContainer<>), replicateDataTr);
            FieldDefinition replicatesHistoryFd = new($"_replicatesHistory___{replicateMd.Name}", FieldAttributes.Private, gh.GetGenericRingBuffer(git));
            typeDef.Fields.Add(replicatesHistoryFd);

            //Replicates queue.
            git = gh.GetGenericType(typeof(ReplicateDataContainer<>), replicateDataTr);
            FieldDefinition replicatesQueueFd = new($"_replicatesQueue___{replicateMd.Name}", FieldAttributes.Private, gh.GetGenericBasicQueue(git));
            typeDef.Fields.Add(replicatesQueueFd);

            //Local reconciles.
            git = gh.GetGenericType(typeof(LocalReconcile<>), reconcileDataTr);
            FieldDefinition localReconcilesFd = new($"_reconcilesHistory___{reconcileMd.Name}", FieldAttributes.Private, gh.GetGenericRingBuffer(git));
            typeDef.Fields.Add(localReconcilesFd);

            //Used for delta reconcile.
            FieldDefinition lastReconcileDataFd = new($"_lastReadReconcile___{reconcileMd.Name}", FieldAttributes.Private, reconcileDataTr);
            typeDef.Fields.Add(lastReconcileDataFd);

            //Used for delta replicates.
            git = gh.GetGenericType(typeof(ReplicateDataContainer<>), replicateDataTr);
            FieldDefinition lastReadReplicateFd = new($"_lastReadReplicate___{replicateMd.Name}", FieldAttributes.Private, git);
            typeDef.Fields.Add(lastReadReplicateFd);

            predictionFields = new()
            {
                ReplicateDataTypeRef = replicateDataTr,
                ReconcileDataTypeRef = reconcileDataTr,

                ReplicateUserLogicDelegate = replicateUserLogicDelegateFd,
                ReconcileUserLogicDelegate = reconcileUserLogicDelegateFd,

                ReplicatesQueue = replicatesQueueFd,
                ReplicatesHistory = replicatesHistoryFd,
                LocalReconciles = localReconcilesFd,

                LastReadReplicate = lastReadReplicateFd,
                LastReadReconcile = lastReconcileDataFd
            };
        }

        /// <summary>
        /// Returns if there are any errors with the prediction methods parameters and will print if so.
        /// </summary>
        private bool HasParameterError(MethodDefinition methodDef, TypeDefinition typeDef, bool replicateMethod)
        {
            //Replicate: data, state, channel.
            //Reconcile: data, asServer, channel.
            int count = replicateMethod ? 3 : 2;

            //Check parameter count.
            if (methodDef.Parameters.Count != count)
            {
                PrintParameterExpectations();
                return true;
            }

            string expectedName;
            //Data check.
            if (!methodDef.Parameters[0].ParameterType.IsClassOrStruct(Session))
            {
                LogError($"Prediction methods must use a class or structure as the first parameter type. Structures are recommended to avoid allocations.");
                return true;
            }

            expectedName = replicateMethod ? typeof(ReplicateState).Name : typeof(Channel).Name;
            if (methodDef.Parameters[1].ParameterType.Name != expectedName)
            {
                PrintParameterExpectations();
                return true;
            }

            //Only replicate uses more than 2 parameters.
            if (replicateMethod)
            {
                //Channel.
                if (methodDef.Parameters[2].ParameterType.Name != typeof(Channel).Name)
                {
                    PrintParameterExpectations();
                    return true;
                }
            }

            void PrintParameterExpectations()
            {
                if (replicateMethod)
                    LogError($"Replicate method {methodDef.Name} within {typeDef.Name} requires exactly {count} parameters. In order: replicate data, state = ReplicateState.Invalid, channel = Channel.Unreliable");
                else
                    LogError($"Reconcile method {methodDef.Name} within {typeDef.Name} requires exactly {count} parameters. In order: reconcile data, channel = Channel.Unreliable.");
            }

            //No errors with parameters.
            return false;
        }

        /// <summary>
        /// Creates all methods needed for a RPC.
        /// </summary>
        /// <returns></returns>
        private bool CreatePredictionMethods(TypeDefinition typeDef, MethodDefinition replicateMd, MethodDefinition reconcileMd, CreatedPredictionFields predictionFields, uint rpcCount, out PredictionReaders predictionReaders, out MethodDefinition replicateULMd, out MethodDefinition reconcileULMd)
        {
            GeneralHelper gh = GetClass<GeneralHelper>();
            NetworkBehaviourHelper nbh = GetClass<NetworkBehaviourHelper>();
            predictionReaders = null;

            uint startingRpcCount = rpcCount;
            string copySuffix = "___UL";
            replicateULMd = GetClass<GeneralHelper>().CopyIntoNewMethod(replicateMd, $"{replicateMd.Name}{copySuffix}", out _);
            reconcileULMd = GetClass<GeneralHelper>().CopyIntoNewMethod(reconcileMd, $"{reconcileMd.Name}{copySuffix}", out _);
            replicateMd.Body.Instructions.Clear();
            reconcileMd.Body.Instructions.Clear();

            TypeReference replicateDataTr = replicateMd.Parameters[0].ParameterType;
            TypeReference reconcileDataTr = reconcileMd.Parameters[0].ParameterType;

            MethodDefinition replicateReader;
            MethodDefinition reconcileReader;

            if (!CreateReplicate())
                return false;
            if (!CreateReconcile())
                return false;
            if (!CreateEmptyReplicatesQueueIntoHistoryStart())
                return false;
            if (!CreateReconcileStart())
                return false;
            if (!CreateReplicateReplayStart())
                return false;

            CreateClearReplicateCacheMethod(typeDef, replicateDataTr, reconcileDataTr, predictionFields);
            CreateReplicateReader(typeDef, startingRpcCount, replicateMd, predictionFields, out replicateReader);
            CreateReconcileReader(typeDef, reconcileMd, predictionFields, out reconcileReader);
            predictionReaders = new(replicateReader, reconcileReader);

            bool CreateReplicate()
            {
                ILProcessor processor = replicateMd.Body.GetILProcessor();
                ParameterDefinition replicateDataPd = replicateMd.Parameters[0];
                replicateDataPd.ParameterType = Module.ImportReference(replicateDataPd.ParameterType);
                MethodDefinition comparerMd = gh.CreateEqualityComparer(replicateDataPd.ParameterType);
                gh.CreateIsDefaultComparer(replicateDataPd.ParameterType, comparerMd);

                ParameterDefinition channelPd = replicateMd.Parameters[2];

                GenericInstanceMethod replicateGim = GetClass<NetworkBehaviourHelper>().Replicate_Current_MethodRef.MakeGenericMethod(new TypeReference[] { replicateDataTr });

                /* ReplicateUserLogicDelegate<T> del
                 * uint methodHash
                 * BasicQueue<ReplicateData<T>> replicatesQueue
                 * RingBuffer<ReplicateData<T>> replicatesHistory
                 * ReplicateData<T> data)
                 *      where T : IReplicateData
                 */
                processor.Emit(OpCodes.Ldarg_0);
                //User logic delegate.
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicateUserLogicDelegate);
                //Rpc hash.
                processor.Emit(OpCodes.Ldc_I4, (int)rpcCount);
                //Replicates queue.
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesQueue);
                //Replicates history.
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);

                /* Data being called into the method. */
                //Generate the ReplicateData<T>
                //new ReplicateData<T>(data, channel)
                processor.Emit(OpCodes.Ldarg, replicateDataPd);
                processor.Emit(OpCodes.Ldarg, channelPd);
                GenericInstanceType git = GetGenericReplicateDataContainer(replicateDataTr);
                MethodReference ctorMr = ReplicateData_Ctor_MethodRef.MakeHostInstanceGeneric(Session, git);
                processor.Emit(OpCodes.Newobj, ctorMr);

                //Call nb.Replicate_Current.
                processor.Emit(OpCodes.Call, replicateGim);

                processor.Emit(OpCodes.Ret);

                return true;
            }


            bool CreateReconcile()
            {
                ILProcessor processor = reconcileMd.Body.GetILProcessor();
                ServerCreateReconcile(reconcileMd, predictionFields, ref rpcCount);
                processor.Emit(OpCodes.Ret);
                return true;
            }

            bool CreateEmptyReplicatesQueueIntoHistoryStart()
            {
                MethodDefinition newMethodDef = nbh.EmptyReplicatesQueueIntoHistory_Start_MethodRef.CachedResolve(Session).CreateCopy(Session, null, MethodDefinitionExtensions.PUBLIC_VIRTUAL_ATTRIBUTES);
                typeDef.Methods.Add(newMethodDef);

                ILProcessor processor = newMethodDef.Body.GetILProcessor();

                MethodReference baseMethodGim = nbh.EmptyReplicatesQueueIntoHistory_MethodRef.GetMethodReference(Session, new TypeReference[] { predictionFields.ReplicateDataTypeRef });

                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesQueue);
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);
                processor.Emit(OpCodes.Call, baseMethodGim);
                processor.Emit(OpCodes.Ret);

                return true;
            }

            //Overrides reconcile start to call reconcile_client_internal.
            bool CreateReconcileStart()
            {
                MethodDefinition newMethodDef = nbh.Reconcile_Client_Start_MethodRef.CachedResolve(Session).CreateCopy(Session, null, MethodDefinitionExtensions.PUBLIC_VIRTUAL_ATTRIBUTES);
                typeDef.Methods.Add(newMethodDef);

                ILProcessor processor = newMethodDef.Body.GetILProcessor();

                Call_Reconcile_Client();

                void Call_Reconcile_Client()
                {
                    MethodReference baseMethodGim = nbh.Reconcile_Client_MethodRef.GetMethodReference(Session, new TypeReference[] { predictionFields.LastReadReconcile.FieldType, predictionFields.ReplicateDataTypeRef });

                    processor.Emit(OpCodes.Ldarg_0);
                    processor.Emit(OpCodes.Ldarg_0);
                    processor.Emit(OpCodes.Ldfld, predictionFields.ReconcileUserLogicDelegate);
                    processor.Emit(OpCodes.Ldarg_0);
                    processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);
                    processor.Emit(OpCodes.Ldarg_0);
                    processor.Emit(OpCodes.Ldfld, predictionFields.LocalReconciles);
                    processor.Emit(OpCodes.Ldarg_0);
                    processor.Emit(OpCodes.Ldfld, predictionFields.LastReadReconcile);
                    processor.Emit(OpCodes.Call, baseMethodGim);
                    processor.Emit(OpCodes.Ret);
                }

                return true;
            }

            bool CreateReplicateReplayStart()
            {
                MethodDefinition newMethodDef = nbh.Replicate_Replay_Start_MethodRef.CachedResolve(Session).CreateCopy(Session, null, MethodDefinitionExtensions.PUBLIC_VIRTUAL_ATTRIBUTES);
                typeDef.Methods.Add(newMethodDef);

                ParameterDefinition replayTickPd = newMethodDef.Parameters[0];
                ILProcessor processor = newMethodDef.Body.GetILProcessor();

                MethodReference baseMethodGim = nbh.Replicate_Replay_MethodRef.GetMethodReference(Session, new TypeReference[] { predictionFields.ReplicateDataTypeRef });

                //uint replicateTick, ReplicateUserLogicDelegate<T> del, List<T> replicates, Channel channel) where T : IReplicateData
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldarg, replayTickPd);
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicateUserLogicDelegate);
                processor.Emit(OpCodes.Ldarg_0);
                processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);
                //processor.Emit(OpCodes.Ldc_I4, (int)Channel.Unreliable); //Channel does not really matter when replaying. At least not until someone needs it.
                processor.Emit(OpCodes.Call, baseMethodGim);
                processor.Emit(OpCodes.Ret);

                return true;
            }

            return true;
        }

        #region Universal prediction.
        /// <summary>
        /// Creates an override for the method responsible for resetting replicates.
        /// </summary>
        /// <param name = ""></param>
        /// <param name = ""></param>
        private void CreateClearReplicateCacheMethod(TypeDefinition typeDef, TypeReference replicateDataTr, TypeReference reconcileDataTr, CreatedPredictionFields predictionFields)
        {
            GeneralHelper gh = GetClass<GeneralHelper>();
            NetworkBehaviourHelper nbh = GetClass<NetworkBehaviourHelper>();

            string methodName = nameof(NetworkBehaviour.ClearReplicateCache);
            MethodDefinition baseClearMd = typeDef.GetMethodDefinitionInAnyBase(Session, methodName);
            MethodDefinition clearMd = typeDef.GetOrCreateMethodDefinition(Session, methodName, baseClearMd, true, out bool created);
            clearMd.Attributes = MethodDefinitionExtensions.PUBLIC_VIRTUAL_ATTRIBUTES;
            //This class already has the method created when it should not.
            if (baseClearMd.DeclaringType == typeDef)
            {
                LogError($"{typeDef.Name} overrides method {methodName} when it should not.");
                return;
            }

            ILProcessor processor = clearMd.Body.GetILProcessor();
            //Call the base class first.
            processor.Emit(OpCodes.Ldarg_0);
            MethodReference baseClearMr = ImportReference(baseClearMd);
            processor.Emit(OpCodes.Call, baseClearMr);

            //Call the actual clear method.
            GenericInstanceMethod internalClearGim = nbh.ClearReplicateCache_Internal_MethodRef.MakeGenericMethod(new[] { replicateDataTr, reconcileDataTr });

            processor.Emit(OpCodes.Ldarg_0); //Base.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesQueue);
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.LocalReconciles);
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldflda, predictionFields.LastReadReplicate);
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldflda, predictionFields.LastReadReconcile);
            processor.Emit(OpCodes.Call, internalClearGim);
            processor.Emit(OpCodes.Ret);
        }

        /// <summary>
        /// Outputs generic ReplicateULDelegate for dataTr.
        /// </summary>
        private void GetGenericULDelegate(TypeReference dataTr, System.Type delegateType, out GenericInstanceType git)
        {
            TypeReference delDataTr = ImportReference(delegateType);
            git = delDataTr.MakeGenericInstanceType(new TypeReference[] { dataTr });
        }

        /// <summary>
        /// Subtracts 1 from a field.
        /// </summary>
        private List<Instruction> SubtractFromField(MethodDefinition methodDef, FieldDefinition fieldDef)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            //      _field--;
            insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Ldarg_0));
            insts.Add(processor.Create(OpCodes.Ldfld, fieldDef));
            insts.Add(processor.Create(OpCodes.Ldc_I4_1));
            insts.Add(processor.Create(OpCodes.Sub));
            insts.Add(processor.Create(OpCodes.Stfld, fieldDef));

            return insts;
        }

        /// <summary>
        /// Subtracts 1 from a variable.
        /// </summary>
        private List<Instruction> SubtractFromVariable(MethodDefinition methodDef, VariableDefinition variableDef)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            //      variable--;
            insts.Add(processor.Create(OpCodes.Ldloc, variableDef));
            insts.Add(processor.Create(OpCodes.Ldc_I4_1));
            insts.Add(processor.Create(OpCodes.Sub));
            insts.Add(processor.Create(OpCodes.Stloc, variableDef));

            return insts;
        }

        /// <summary>
        /// Subtracts 1 from a variable.
        /// </summary>
        private List<Instruction> SubtractOneVariableFromAnother(MethodDefinition methodDef, VariableDefinition srcVd, VariableDefinition modifierVd)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            //      variable -= v2;
            insts.Add(processor.Create(OpCodes.Ldloc, srcVd));
            insts.Add(processor.Create(OpCodes.Ldloc, modifierVd));
            insts.Add(processor.Create(OpCodes.Sub));
            insts.Add(processor.Create(OpCodes.Stloc, srcVd));

            return insts;
        }
        #endregion

        #region Server side.
        /// <summary>
        /// Creates a reader for replicate data received from clients.
        /// </summary>
        private bool CreateReplicateReader(TypeDefinition typeDef, uint hash, MethodDefinition replicateMd, CreatedPredictionFields predictionFields, out MethodDefinition result)
        {
            string methodName = $"{REPLICATE_READER_PREFIX}{replicateMd.Name}";
            MethodDefinition createdMd = new(methodName, MethodAttributes.Private, replicateMd.Module.TypeSystem.Void);
            typeDef.Methods.Add(createdMd);
            createdMd.Body.InitLocals = true;

            ILProcessor processor = createdMd.Body.GetILProcessor();

            GeneralHelper gh = GetClass<GeneralHelper>();
            NetworkBehaviourHelper nbh = GetClass<NetworkBehaviourHelper>();

            TypeReference dataTr = replicateMd.Parameters[0].ParameterType;
            //Create parameters.
            ParameterDefinition readerPd = gh.CreateParameter(createdMd, typeof(PooledReader));
            ParameterDefinition networkConnectionPd = gh.CreateParameter(createdMd, typeof(NetworkConnection));
            ParameterDefinition channelPd = gh.CreateParameter(createdMd, typeof(Channel));

            MethodReference replicateReaderGim = nbh.Replicate_Reader_MethodRef.GetMethodReference(Session, dataTr);

            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldc_I4, (int)hash);
            //Reader, NetworkConnection.
            processor.Emit(OpCodes.Ldarg, readerPd);
            processor.Emit(OpCodes.Ldarg, networkConnectionPd);
            //lastFirstReadReplicate.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldflda, predictionFields.LastReadReplicate);
            //Replicates queue.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesQueue);
            //Replicates history.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.ReplicatesHistory);
            //Channel.
            processor.Emit(OpCodes.Ldarg, channelPd);
            processor.Emit(OpCodes.Call, replicateReaderGim);

            processor.Emit(OpCodes.Ret);
            result = createdMd;
            return true;
        }

        /// <summary>
        /// Creates server side code for reconcileMd.
        /// </summary>
        /// <param name = "reconcileMd"></param>
        /// <returns></returns>
        private void ServerCreateReconcile(MethodDefinition reconcileMd, CreatedPredictionFields predictionFields, ref uint rpcCount)
        {
            ParameterDefinition reconcileDataPd = reconcileMd.Parameters[0];
            reconcileDataPd.ParameterType = Module.ImportReference(reconcileDataPd.ParameterType);
            ParameterDefinition channelPd = reconcileMd.Parameters[1];
            ILProcessor processor = reconcileMd.Body.GetILProcessor();

            NetworkBehaviourHelper nbh = GetClass<NetworkBehaviourHelper>();
            GenericInstanceMethod methodGim;

            /* Reconcile_Current. */
            methodGim = nbh.Reconcile_Current_MethodRef.MakeGenericMethod(new TypeReference[] { reconcileDataPd.ParameterType });

            //Hash.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldc_I4, (int)rpcCount);
            //Last reconcile data.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldflda, predictionFields.LastReadReconcile);
            //Reconciles history (local reconciles).
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldfld, predictionFields.LocalReconciles);
            //Data.
            processor.Emit(OpCodes.Ldarg, reconcileDataPd);
            //Channel.
            processor.Emit(OpCodes.Ldarg, channelPd);

            processor.Emit(OpCodes.Call, methodGim);

            rpcCount++;
        }
        #endregion

        #region Client side.
        /// <summary>
        /// Creates a reader for replicate data received from clients.
        /// </summary>
        private bool CreateReconcileReader(TypeDefinition typeDef, MethodDefinition reconcileMd, CreatedPredictionFields predictionFields, out MethodDefinition result)
        {
            string methodName = $"{RECONCILE_READER_PREFIX}{reconcileMd.Name}";
            MethodDefinition createdMd = new(methodName, MethodAttributes.Private, reconcileMd.Module.TypeSystem.Void);
            typeDef.Methods.Add(createdMd);
            createdMd.Body.InitLocals = true;

            ILProcessor processor = createdMd.Body.GetILProcessor();

            GeneralHelper gh = GetClass<GeneralHelper>();
            NetworkBehaviourHelper nbh = GetClass<NetworkBehaviourHelper>();

            TypeReference dataTr = reconcileMd.Parameters[0].ParameterType;
            //Create parameters.
            ParameterDefinition readerPd = gh.CreateParameter(createdMd, typeof(PooledReader));
            ParameterDefinition channelPd = gh.CreateParameter(createdMd, typeof(Channel));

            MethodReference methodGim = nbh.Reconcile_Reader_MethodRef.GetMethodReference(Session, dataTr);

            processor.Emit(OpCodes.Ldarg_0);

            /* nb.Reconcile_Reader(readerPd, ref lastReadReconcile); */
            processor.Emit(OpCodes.Ldarg, readerPd);
            //Data to assign read value to.
            processor.Emit(OpCodes.Ldarg_0);
            processor.Emit(OpCodes.Ldflda, predictionFields.LastReadReconcile);

            processor.Emit(OpCodes.Call, methodGim);

            //Add end of method.
            processor.Emit(OpCodes.Ret);

            result = createdMd;
            return true;
        }

        /// <summary>
        /// Outputs generic RingBuffer for dataTr.
        /// </summary>
        public GenericInstanceType GetGenericReplicateDataContainer(TypeReference dataTr)
        {
            TypeReference typeTr = ImportReference(typeof(ReplicateDataContainer<>));
            return typeTr.MakeGenericInstanceType(new TypeReference[] { dataTr });
        }
        #endregion
    }
}