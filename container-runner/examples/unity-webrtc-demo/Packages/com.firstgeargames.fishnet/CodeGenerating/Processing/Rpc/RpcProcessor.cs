using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Configuring;
using FishNet.Connection;
using FishNet.Managing.Logging;
using FishNet.Object.Helping;
using FishNet.Transporting;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using GameKit.Dependencies.Utilities;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using FishNet.Object;
using MonoFN.Collections.Generic;
using UnityEngine;

namespace FishNet.CodeGenerating.Processing.Rpc
{
    internal class RpcProcessor : CodegenBase
    {
        #region Types.
        private struct DelegateData
        {
            public RpcType RpcType;
            public bool RunLocally;
            public MethodDefinition OriginalMethodDef;
            public MethodDefinition ReaderMethodDef;
            public uint MethodHash;
            public CustomAttribute RpcAttribute;

            public DelegateData(RpcType rpcType, bool runLocally, MethodDefinition originalMethodDef, MethodDefinition readerMethodDef, uint methodHash, CustomAttribute rpcAttribute)
            {
                RpcType = rpcType;
                RunLocally = runLocally;
                OriginalMethodDef = originalMethodDef;
                ReaderMethodDef = readerMethodDef;
                MethodHash = methodHash;
                RpcAttribute = rpcAttribute;
            }
        }
        #endregion

        #region Public.
        /// <summary>
        /// Attribute helper.
        /// </summary>
        public Attributes Attributes = new();
        #endregion

        private List<(MethodDefinition, MethodDefinition)> _virtualRpcs = new List<(MethodDefinition createdLogicMd, MethodDefinition originalRpcMd)>();

        #region Const.
        private const string LOGIC_PREFIX = "RpcLogic___";
        private const string WRITER_PREFIX = "RpcWriter___";
        private const string READER_PREFIX = "RpcReader___";
        private const string REQUIREOWNERSHIP_NAME = nameof(ServerRpcAttribute.RequireOwnership);
        private const string RUNLOCALLY_NAME = nameof(RpcAttribute.RunLocally);
        private const string EXCLUDEOWNER_NAME = nameof(ObserversRpcAttribute.ExcludeOwner);
        private const string EXCLUDESERVER_NAME = nameof(TargetRpcAttribute.ExcludeServer);
        private const string BUFFERLAST_NAME = nameof(ObserversRpcAttribute.BufferLast);
        private const string DATALENGTH_NAME = nameof(RpcAttribute.DataLength);
        private const string VALIDATETARGET_NAME = nameof(TargetRpcAttribute.ValidateTarget);
        private const string DATAORDERTYPE_NAME = nameof(RpcAttribute.OrderType);
        private const string LOGGING_NAME = nameof(ServerRpcAttribute.Logging);
        #endregion

        public override bool ImportReferences()
        {
            Attributes.Initialize(Session);
            return base.ImportReferences();
        }

        internal bool ProcessLocal(TypeDefinition typeDef)
        {
            bool modified = false;

            PredictionProcessor pp = GetClass<PredictionProcessor>();
            uint rpcCount = GetRpcCountInParents(typeDef) + pp.GetPredictionCountInParents(typeDef) + pp.GetPredictionCount(typeDef);
            // All createdRpcs for typeDef.
            List<CreatedRpc> typeDefCeatedRpcs = new();
            List<MethodDefinition> methodDefs = typeDef.Methods.ToList();
            foreach (MethodDefinition md in methodDefs)
            {
                if (rpcCount >= NetworkBehaviourHelper.MAX_RPC_ALLOWANCE)
                {
                    LogError($"{typeDef.FullName} and inherited types exceed {NetworkBehaviourHelper.MAX_RPC_ALLOWANCE} RPC methods. Only {NetworkBehaviourHelper.MAX_RPC_ALLOWANCE} RPC methods are supported per inheritance hierarchy.");
                    return false;
                }

                // Rpcs created for this method.
                List<CreatedRpc> createdRpcs = new();
                List<AttributeData> attributeDatas = Attributes.GetRpcAttributes(md);
                bool success = true;
                foreach (AttributeData ad in attributeDatas)
                {
                    CreatedRpc cr = new();
                    cr.OriginalMethodDef = md;
                    cr.AttributeData = ad;
                    cr.MethodHash = rpcCount;

                    /* This is a one time check to make sure the rpcType is
                     * a supported value. Multiple methods beyond this rely on the
                     * value being supported. Rather than check in each method a
                     * single check is performed here. */
                    if (cr.RpcType != RpcType.Observers && cr.RpcType != RpcType.Server && cr.RpcType != RpcType.Target)
                    {
                        LogError($"RpcType of {cr.RpcType.ToString()} is unhandled.");
                        break;
                    }

                    bool created = CreateRpcMethods(attributeDatas, cr);
                    if (created)
                    {
                        modified = true;

                        typeDefCeatedRpcs.Add(cr);
                        createdRpcs.Add(cr);

                        if (cr.LogicMethodDef != null && cr.LogicMethodDef.IsVirtual)
                            _virtualRpcs.Add((cr.LogicMethodDef, md));

                        rpcCount++;
                    }
                    else
                    {
                        success = false;
                    }
                }

                // If at least one attribute was found and all rpc methods were made.   
                if (createdRpcs.Count > 0 && success)
                    RedirectOriginalToWriter(createdRpcs);
            }

            if (modified)
            {
                foreach (CreatedRpc cr in typeDefCeatedRpcs)
                    GetClass<NetworkBehaviourHelper>().CreateRpcDelegate(cr.RunLocally, cr.TypeDef, cr.ReaderMethodDef, cr.RpcType, cr.MethodHash, cr.Attribute);

                return true;
            }
            else
            {
                return false;
            }
        }

        /// <summary>
        /// Returns the name to use for a RpcMethod.
        /// </summary>
        private string GetRpcMethodName(CreatedRpc cr)
        {
            return GetRpcMethodName(cr.RpcType, cr.OriginalMethodDef);
        }

        /// <summary>
        /// Returns the name to use for a RpcMethod.
        /// </summary>
        private string GetRpcMethodName(RpcType rpcType, MethodDefinition originalMd)
        {
            return $"{rpcType.ToString()}_{GetMethodNameAsParameters(originalMd)}";
        }

        /// <summary>
        /// Gets RPCcount count in all of typeDefs parents, excluding typeDef itself.
        /// </summary>
        internal uint GetRpcCountInParents(TypeDefinition typeDef)
        {
            uint count = 0;
            do
            {
                typeDef = typeDef.GetNextBaseClassToProcess(Session);
                if (typeDef != null)
                    count += GetRpcCount(typeDef);
            } while (typeDef != null);

            return count;
        }

        /// <summary>
        /// Returns the method name with parameter types included within the name.
        /// </summary>
        public static string GetMethodNameAsParameters(MethodDefinition methodDef)
        {
            StringBuilder sb = new();
            foreach (ParameterDefinition pd in methodDef.Parameters)
                sb.Append(pd.ParameterType.FullName);

            string result = $"{methodDef.Name}___{sb.ToString().GetStableHashU32()}";
            return result;
        }

        /// <summary>
        /// Redirects base calls for overriden RPCs.
        /// </summary>
        internal void RedirectBaseCalls()
        {
            foreach ((MethodDefinition logicMd, MethodDefinition originalMd) in _virtualRpcs)
                RedirectBaseCall(logicMd, originalMd);
        }

        /// <summary>
        /// Gets number of RPCs by checking for RPC attributes. This does not perform error checking.
        /// </summary>
        /// <param name = "typeDef"></param>
        /// <returns></returns>
        internal uint GetRpcCount(TypeDefinition typeDef)
        {
            uint count = 0;
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                foreach (CustomAttribute customAttribute in methodDef.CustomAttributes)
                {
                    RpcType rpcType = GetClass<AttributeHelper>().GetRpcAttributeType(customAttribute);
                    if (rpcType != RpcType.None)
                        count++;
                }
            }

            return count;
        }

        /// <summary>
        /// Creates all methods needed for a RPC.
        /// </summary>
        /// <param name = "originalMd"></param>
        /// <param name = "rpcAttribute"></param>
        /// <returns>True if successful.</returns>
        private bool CreateRpcMethods(List<AttributeData> datas, CreatedRpc cr)
        {
            cr.RunLocally = cr.Attribute.GetField(RUNLOCALLY_NAME, false);
            bool intentionallyNull;

            List<ParameterDefinition> serializedParameters = GetSerializedParamters(cr.RpcType, datas, cr);

            cr.WriterMethodDef = CreateRpcWriterMethod(serializedParameters, datas, cr, out intentionallyNull);
            if (!intentionallyNull && cr.WriterMethodDef == null)
                return false;

            cr.LogicMethodDef = CreateRpcLogicMethod(datas, cr, out intentionallyNull);
            if (!intentionallyNull && cr.LogicMethodDef == null)
                return false;

            cr.ReaderMethodDef = CreateRpcReaderMethod(serializedParameters, datas, cr, out intentionallyNull);
            if (!intentionallyNull && cr.ReaderMethodDef == null)
                return false;

            return true;
        }

        /// <summary>
        /// Creates a writer for a RPC.
        /// </summary>
        private MethodDefinition CreateRpcWriterMethod(List<ParameterDefinition> serializedParameters, List<AttributeData> datas, CreatedRpc cr, out bool intentionallyNull)
        {
            intentionallyNull = false;


            string methodName = $"{WRITER_PREFIX}{GetRpcMethodName(cr)}";
            /* If method already exist then clear it. This
             * can occur when a method needs to be rebuilt due to
             * inheritence, and renumbering the RPC method names. */
            MethodDefinition createdMd = cr.TypeDef.GetMethod(methodName);
            // If found.
            if (createdMd != null)
            {
                createdMd.Parameters.Clear();
                createdMd.Body.Instructions.Clear();
            }
            // Doesn't exist, create it.
            else
            {
                // Create the method body.
                createdMd = new(methodName, MethodAttributes.Private, cr.Module.TypeSystem.Void);
                cr.TypeDef.Methods.Add(createdMd);
                createdMd.Body.InitLocals = true;
            }
            cr.WriterMethodDef = createdMd;

            bool result;
            if (cr.RpcType == RpcType.Server)
                result = CreateServerRpcWriterMethod(serializedParameters, cr);
            else if (cr.RpcType == RpcType.Target || cr.RpcType == RpcType.Observers)
                result = CreateClientRpcWriterMethod(serializedParameters, datas, cr);
            else
                result = false;

            return result ? cr.WriterMethodDef : null;
        }

        /// <summary>
        /// Returns serializable parameters for originalMd.
        /// </summary>
        private List<ParameterDefinition> GetSerializedParamters(RpcType rpcType, List<AttributeData> attributeDatas, CreatedRpc cr)
        {
            MethodDefinition originalMd = cr.OriginalMethodDef;

            // RpcTypes for originalMd.
            List<RpcType> attributeRpcTypes = attributeDatas.GetRpcTypes();

            // Parameters to be serialized.
            List<ParameterDefinition> serializedParameters = new();
            /* Parameters which won't be serialized, such as channel.
             * It's safe to add parameters which are null or
             * not used. */
            HashSet<ParameterDefinition> nonserializedParameters = new();

            // Get channel if it exist, and get target parameter.
            ParameterDefinition channelParameterDef = GetChannelParameter(originalMd, rpcType);

            /* RpcType specific parameters. */
            ParameterDefinition targetConnectionParameterDef = null;
            if (attributeRpcTypes.Contains(RpcType.Target))
                targetConnectionParameterDef = originalMd.Parameters[0];

            if (rpcType == RpcType.Server)
            {
                // The network connection parameter might be added as null, this is okay.
                nonserializedParameters.Add(GetNetworkConnectionParameter(originalMd));
                nonserializedParameters.Add(channelParameterDef);
            }
            else
            {
                nonserializedParameters.Add(channelParameterDef);
                nonserializedParameters.Add(targetConnectionParameterDef);
            }

            // Add all parameters which are NOT non-serialized to serializedParameters.
            foreach (ParameterDefinition pd in originalMd.Parameters)
            {
                if (!nonserializedParameters.Contains(pd))
                    serializedParameters.Add(pd);
            }

            return serializedParameters;
        }

        /// <summary>
        /// Creates Writer method for a TargetRpc.
        /// </summary>
        private bool CreateClientRpcWriterMethod(List<ParameterDefinition> serializedParameters, List<AttributeData> attributeDatas, CreatedRpc cr)
        {
            WriterProcessor wp = GetClass<WriterProcessor>();

            MethodDefinition writerMd = cr.WriterMethodDef;
            MethodDefinition originalMd = cr.OriginalMethodDef;

            ILProcessor processor = writerMd.Body.GetILProcessor();
            // Add all parameters from the original.
            for (int i = 0; i < originalMd.Parameters.Count; i++)
                writerMd.Parameters.Add(originalMd.Parameters[i].CloneImported(Session, writerMd));
            // Get channel if it exist, and get target parameter.
            ParameterDefinition channelParameterDef = GetChannelParameter(writerMd, RpcType.None);

            List<RpcType> rpcTypes = attributeDatas.GetRpcTypes();

            /* RpcType specific parameters. */
            ParameterDefinition targetConnectionParameterDef = null;
            if (rpcTypes.Contains(RpcType.Target))
                targetConnectionParameterDef = writerMd.Parameters[0];

            /* Creates basic ServerRpc and ClientRpc
             * conditions such as if requireOwnership ect..
             * or if (!base.isClient) */
                CreateClientRpcConditionsForServer(writerMd, cr.Attribute);

            VariableDefinition channelVariableDef = CreateAndPopulateChannelVariable(writerMd, channelParameterDef);
            /* Create a local PooledWriter variable. */
            // Default value for data length.
            int dataLength = -1;
            // Go through each attribute and see if a larger data length is specified.
            foreach (AttributeData ad in attributeDatas)
            {
                int dl = ad.Attribute.GetField(DATALENGTH_NAME, -1);
                if (dl > dataLength)
                    dataLength = dl;
            }
            VariableDefinition pooledWriterVariableDef = wp.CreatePooledWriter(writerMd, dataLength);
            // Create all writer.WriteType() calls. 
            for (int i = 0; i < serializedParameters.Count; i++)
            {
                MethodReference writeMethodRef = wp.GetOrCreateWriteMethodReference(serializedParameters[i].ParameterType, MethodDefinitionTraceText(cr.OriginalMethodDef));
                if (writeMethodRef == null)
                    return false;

                wp.CreateWrite(writerMd, pooledWriterVariableDef, serializedParameters[i], writeMethodRef);
            }

            /* Call the method on NetworkBehaviour responsible for sending out the rpc. */
            if (cr.RpcType == RpcType.Observers)
                processor.Add(CreateSendObserversRpc(writerMd, cr.MethodHash, pooledWriterVariableDef, channelVariableDef, cr.Attribute));
            else if (cr.RpcType == RpcType.Target)
                processor.Add(CreateSendTargetRpc(writerMd, cr.MethodHash, pooledWriterVariableDef, channelVariableDef, targetConnectionParameterDef, attributeDatas));
            // Dispose of writer.
            processor.Add(GetClass<WriterProcessor>().DisposePooledWriter(writerMd, pooledWriterVariableDef));
            // Add end of method.
            processor.Emit(OpCodes.Ret);

            return true;
        }

        /// <summary>
        /// Creates Writer method for a ServerRpc.
        /// </summary>
        private bool CreateServerRpcWriterMethod(List<ParameterDefinition> serializedParameters, CreatedRpc cr)
        {
            WriterProcessor wp = GetClass<WriterProcessor>();

            MethodDefinition writerMd = cr.WriterMethodDef;
            MethodDefinition originalMd = cr.OriginalMethodDef;
            ILProcessor processor = writerMd.Body.GetILProcessor();

            // Add all parameters from the original.
            for (int i = 0; i < originalMd.Parameters.Count; i++)
                writerMd.Parameters.Add(originalMd.Parameters[i].CloneImported(Session, writerMd));
            // Add in channel if it doesnt exist.
            ParameterDefinition channelParameterDef = GetChannelParameter(writerMd, RpcType.Server);

            /* Creates basic ServerRpc
             * conditions such as if requireOwnership ect..
             * or if (!base.isClient) */
                CreateServerRpcConditionsForClient(writerMd, cr.Attribute);

            VariableDefinition channelVariableDef = CreateAndPopulateChannelVariable(writerMd, channelParameterDef);
            // Create a local PooledWriter variable.
            int dataLength = cr.Attribute.GetField(DATALENGTH_NAME, -1);
            VariableDefinition pooledWriterVariableDef = wp.CreatePooledWriter(writerMd, dataLength);
            // Create all writer.WriteType() calls. 
            for (int i = 0; i < serializedParameters.Count; i++)
            {
                MethodReference writeMethodRef = wp.GetOrCreateWriteMethodReference(serializedParameters[i].ParameterType, MethodDefinitionTraceText(cr.OriginalMethodDef));
                if (writeMethodRef == null)
                    return false;

                wp.CreateWrite(writerMd, pooledWriterVariableDef, serializedParameters[i], writeMethodRef);
            }

            // Call the method on NetworkBehaviour responsible for sending out the rpc.
            processor.Add(CreateSendServerRpc(writerMd, cr.MethodHash, pooledWriterVariableDef, channelVariableDef, cr.Attribute));
            // Dispose of writer.
            processor.Add(wp.DisposePooledWriter(writerMd, pooledWriterVariableDef));
            // Add end of method.
            processor.Emit(OpCodes.Ret);

            return true;
        }

        /// <summary>
        /// Creates a Channel VariableDefinition and populates it with parameterDef value if available, otherwise uses Channel.Reliable.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <param name = "parameterDef"></param>
        /// <returns></returns>
        private VariableDefinition CreateAndPopulateChannelVariable(MethodDefinition methodDef, ParameterDefinition parameterDef)
        {
            ILProcessor processor = methodDef.Body.GetILProcessor();

            VariableDefinition localChannelVariableDef = GetClass<GeneralHelper>().CreateVariable(methodDef, typeof(Channel));
            if (parameterDef != null)
                processor.Emit(OpCodes.Ldarg, parameterDef);
            else
                processor.Emit(OpCodes.Ldc_I4, (int)Channel.Reliable);

            // Set to local value.
            processor.Emit(OpCodes.Stloc, localChannelVariableDef);
            return localChannelVariableDef;
        }

        /// <summary>
        /// Creates a reader for a RPC.
        /// </summary>
        /// <param name = "originalMd"></param>
        /// <param name = "rpcAttribute"></param>
        /// <returns></returns>
        private MethodDefinition CreateRpcReaderMethod(List<ParameterDefinition> serializedParameters, List<AttributeData> datas, CreatedRpc cr, out bool intentionallyNull)
        {
            intentionallyNull = false;

            RpcType rpcType = cr.RpcType;
            MethodDefinition originalMd = cr.OriginalMethodDef;
            TypeDefinition typeDef = cr.TypeDef;
            bool runLocally = cr.RunLocally;
            MethodDefinition logicMd = cr.LogicMethodDef;
            CustomAttribute rpcAttribute = cr.Attribute;


            string methodName = $"{READER_PREFIX}{GetRpcMethodName(cr)}";
            /* If method already exist then just return it. This
             * can occur when a method needs to be rebuilt due to
             * inheritence, and renumbering the RPC method names.
             * The reader method however does not need to be rewritten. */
            MethodDefinition createdMd = typeDef.GetMethod(methodName);
            // If found.
            if (createdMd != null)
            {
                cr.ReaderMethodDef = createdMd;
                return createdMd;
            }
            else
            {
                // Create the method body.
                createdMd = new(methodName, MethodAttributes.Private, originalMd.Module.TypeSystem.Void);
                typeDef.Methods.Add(createdMd);
                createdMd.Body.InitLocals = true;
                cr.ReaderMethodDef = createdMd;
            }

            if (rpcType == RpcType.Server)
                return CreateServerRpcReaderMethod(typeDef, runLocally, originalMd, createdMd, serializedParameters, logicMd, rpcAttribute);
            else if (rpcType == RpcType.Target || rpcType == RpcType.Observers)
                return CreateClientRpcReaderMethod(serializedParameters, datas, cr);
            else
                return null;
        }

        /// <summary>
        /// Creates a reader for ServerRpc.
        /// </summary>
        /// <param name = "originalMd"></param>
        /// <param name = "rpcAttribute"></param>
        /// <returns></returns>
        private MethodDefinition CreateServerRpcReaderMethod(TypeDefinition typeDef, bool runLocally, MethodDefinition originalMd, MethodDefinition createdMd, List<ParameterDefinition> serializedParameters, MethodDefinition logicMd, CustomAttribute rpcAttribute)
        {
            ILProcessor processor = createdMd.Body.GetILProcessor();

            bool requireOwnership = rpcAttribute.GetField(REQUIREOWNERSHIP_NAME, true);
            // Create PooledReader parameter.
            ParameterDefinition readerParameterDef = GetClass<GeneralHelper>().CreateParameter(createdMd, GetClass<ReaderImports>().PooledReader_TypeRef);

            // Add connection parameter to the read method. Internals pass the connection into this.
            ParameterDefinition channelParameterDef = GetOrCreateChannelParameter(createdMd, RpcType.Server);
            ParameterDefinition connectionParameterDef = GetOrCreateNetworkConnectionParameter(createdMd);

            /* It's very important to read everything
             * from the PooledReader before applying any
             * exit logic. Should the method return before
             * reading the data then anything after the rpc
             * packet will be malformed due to invalid index. */
            VariableDefinition[] readVariableDefs;
            List<Instruction> allReadInsts;
            CreateRpcReadInstructions(createdMd, readerParameterDef, serializedParameters, out readVariableDefs, out allReadInsts);

            // Read to clear pooledreader.
            processor.Add(allReadInsts);

            /* Don't continue if server is not active.
             * This can happen if an object is deinitializing
             * as a RPC arrives. When separate server and client
             * this should not occur but there's a chance as host
             * because deinitializations are slightly delayed to support
             * the clientHost deinitializing the object as well. */
            GetClass<NetworkBehaviourHelper>().CreateIsServerCheck(createdMd, LoggingType.Off, false, false, false);
            //
            CreateServerRpcConditionsForServer(processor, requireOwnership, connectionParameterDef);

            //Block from running twice as host.
            if (runLocally)
            {
                //The connection calling is always passed into the reader method as the last parameter.
                ParameterDefinition ncPd = createdMd.Parameters[createdMd.Parameters.Count - 1];
                Instruction afterConnectionRet = processor.Create(OpCodes.Nop);
                processor.Emit(OpCodes.Ldarg, ncPd);
                MethodReference isLocalClientMr = GetClass<ObjectHelper>().NetworkConnection_GetIsLocalClient_MethodRef;
                processor.Emit(isLocalClientMr.GetCallOpCode(Session), isLocalClientMr);
                processor.Emit(OpCodes.Brfalse_S, afterConnectionRet);
                processor.Emit(OpCodes.Ret);
                processor.Append(afterConnectionRet);
            }

            //this.Logic
            processor.Emit(OpCodes.Ldarg_0);
            //Add each read variable as an argument. 
            foreach (VariableDefinition vd in readVariableDefs)
                processor.Emit(OpCodes.Ldloc, vd);

            /* Pass in channel and connection if original
             * method supports them. */
            ParameterDefinition originalChannelParameterDef = GetChannelParameter(originalMd, RpcType.Server);
            ParameterDefinition originalConnectionParameterDef = GetNetworkConnectionParameter(originalMd);
            if (originalChannelParameterDef != null)
                processor.Emit(OpCodes.Ldarg, channelParameterDef);
            if (originalConnectionParameterDef != null)
                processor.Emit(OpCodes.Ldarg, connectionParameterDef);

            //Call __Logic method.
            MethodReference logicMr = logicMd.GetMethodReference(Session);
            processor.Emit(OpCodes.Call, logicMr);
            processor.Emit(OpCodes.Ret);

            return createdMd;
        }

        /// <summary>
        /// Creates a reader for ObserversRpc.
        /// </summary>
        /// <param name = "originalMd"></param>
        /// <param name = "rpcAttribute"></param>
        /// <returns></returns>
        private MethodDefinition CreateClientRpcReaderMethod(List<ParameterDefinition> serializedParameters, List<AttributeData> attributeDatas, CreatedRpc cr)
        {
            MethodDefinition originalMd = cr.OriginalMethodDef;
            MethodDefinition createdMd = cr.ReaderMethodDef;
            RpcType rpcType = cr.RpcType;
            CustomAttribute rpcAttribute = cr.Attribute;
            bool runLocally = cr.RunLocally;

            ILProcessor processor = createdMd.Body.GetILProcessor();

            //Create PooledReader parameter.
            ParameterDefinition readerParameterDef = GetClass<GeneralHelper>().CreateParameter(createdMd, GetClass<ReaderImports>().PooledReader_TypeRef);
            ParameterDefinition channelParameterDef = GetOrCreateChannelParameter(createdMd, rpcType);
            /* It's very important to read everything
             * from the PooledReader before applying any
             * exit logic. Should the method return before
             * reading the data then anything after the rpc
             * packet will be malformed due to invalid index. */
            VariableDefinition[] readVariableDefs;
            List<Instruction> allReadInsts;
            CreateRpcReadInstructions(createdMd, readerParameterDef, serializedParameters, out readVariableDefs, out allReadInsts);
            //Read instructions even if not to include owner.
            processor.Add(allReadInsts);

            /* Don't continue if client is not active.
             * This can happen if an object is deinitializing
             * as a RPC arrives. When separate server and client
             * this should not occur but there's a chance as host
             * because deinitializations are slightly delayed to support
             * the clientHost deinitializing the object as well. */
            GetClass<NetworkBehaviourHelper>().CreateIsClientCheck(createdMd, LoggingType.Off, false, false, false);

            //Block from running twice as host.
            if (runLocally)
                processor.Add(CreateIsHostBlock(createdMd));

            processor.Emit(OpCodes.Ldarg_0); //this.
            /* TargetRpc passes in localconnection
             * as receiver for connection. */
            if (rpcType == RpcType.Target)
            {
                processor.Emit(OpCodes.Ldarg_0); //this.
                processor.Emit(OpCodes.Call, GetClass<NetworkBehaviourHelper>().LocalConnection_MethodRef);
            }
            else
            {
                //If this method uses target/observerRpc combined then load null for the connection.
                RpcType allRpcTypes = attributeDatas.GetCombinedRpcType();
                if (allRpcTypes == (RpcType.Observers | RpcType.Target))
                    processor.Emit(OpCodes.Ldnull);
            }
            //Add each read variable as an argument. 
            foreach (VariableDefinition vd in readVariableDefs)
                processor.Emit(OpCodes.Ldloc, vd);
            //Channel.
            ParameterDefinition originalChannelParameterDef = GetChannelParameter(originalMd, rpcType);
            if (originalChannelParameterDef != null)
                processor.Emit(OpCodes.Ldarg, channelParameterDef);

            //Call __Logic method.
            //MethodReference logicMr = cr.LogicMethodDef.GetMethodReference(base.Session);
            processor.Emit(OpCodes.Call, cr.LogicMethodDef);
            processor.Emit(OpCodes.Ret);

            return createdMd;
        }

        /// <summary>
        /// Appends a block to the method if running as host.
        /// </summary>
        /// <param name = "md"></param>
        private List<Instruction> CreateIsHostBlock(MethodDefinition md)
        {
            List<Instruction> ints = new();
            ILProcessor processor = md.Body.GetILProcessor();

            Instruction endIfInst = processor.Create(OpCodes.Nop);
            ints.Add(processor.Create(OpCodes.Ldarg_0));
            ints.Add(processor.Create(OpCodes.Call, GetClass<NetworkBehaviourHelper>().IsHost_MethodRef));
            ints.Add(processor.Create(OpCodes.Brfalse_S, endIfInst));
            ints.Add(processor.Create(OpCodes.Ret));
            ints.Add(endIfInst);

            return ints;
        }

        /// <summary>
        /// Gets the optional NetworkConnection parameter for ServerRpc, if it exists.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <returns></returns>
        private ParameterDefinition GetNetworkConnectionParameter(MethodDefinition methodDef)
        {
            ParameterDefinition result = methodDef.GetEndParameter(0);
            //Is null, not networkconnection, or doesn't have default.
            if (result == null || !result.Is(typeof(NetworkConnection)) || !result.HasDefault)
                return null;

            return result;
        }

        /// <summary>
        /// Creates a NetworkConnection parameter if it's not the last or second to last parameter.
        /// </summary>
        /// <param name = "methodDef"></param>
        private ParameterDefinition GetOrCreateNetworkConnectionParameter(MethodDefinition methodDef)
        {
            ParameterDefinition result = GetNetworkConnectionParameter(methodDef);
            if (result == null)
                return GetClass<GeneralHelper>().CreateParameter(methodDef, typeof(NetworkConnection), "conn");
            else
                return result;
        }

        /// <summary>
        /// Returns the Channel parameter if it exist.
        /// </summary>
        /// <param name = "originalMethodDef"></param>
        private ParameterDefinition GetChannelParameter(MethodDefinition methodDef, RpcType rpcType)
        {
            ParameterDefinition result = null;
            ParameterDefinition pd = methodDef.GetEndParameter(0);
            if (pd != null)
            {
                //Last parameter is channel.
                if (pd.Is(typeof(Channel)))
                {
                    result = pd;
                }
                /* Only other end parameter may be networkconnection.
                 * This can only be checked if a ServerRpc. */
                else if (rpcType == RpcType.Server)
                {
                    //If last parameter is networkconnection and its default then can check second to last.
                    if (pd.Is(typeof(NetworkConnection)) && pd.HasDefault)
                    {
                        pd = methodDef.GetEndParameter(1);
                        if (pd != null && pd.Is(typeof(Channel)))
                            result = pd;
                    }
                }
                else
                {
                    result = null;
                }
            }

            return result;
        }

        /// <summary>
        /// Creates a channel parameter if missing.
        /// </summary>
        /// <param name = "originalMethodDef"></param>
        private ParameterDefinition GetOrCreateChannelParameter(MethodDefinition methodDef, RpcType rpcType)
        {
            ParameterDefinition result = GetChannelParameter(methodDef, rpcType);
            //Add channel parameter if not included.
            if (result == null)
            {
                ParameterDefinition connParameter = GetNetworkConnectionParameter(methodDef);
                //If the connection parameter is specified then channel has to go before it.
                if (connParameter != null)
                    return GetClass<GeneralHelper>().CreateParameter(methodDef, typeof(Channel), "channel", ParameterAttributes.None, connParameter.Index);
                //Not specified, add channel at end.
                else
                    return GetClass<GeneralHelper>().CreateParameter(methodDef, typeof(Channel), "channel");
            }
            else
            {
                return result;
            }
        }

        /// <summary>
        /// Creates a read for every writtenParameters and outputs variables read into, and instructions.
        /// </summary>
        /// <param name = "processor"></param>
        /// <param name = "methodDef"></param>
        /// <param name = "readerParameterDef"></param>
        /// <param name = "serializedParameters"></param>
        /// <param name = "readVariableDefs"></param>
        /// <param name = "allReadInsts"></param>
        private void CreateRpcReadInstructions(MethodDefinition methodDef, ParameterDefinition readerParameterDef, List<ParameterDefinition> serializedParameters, out VariableDefinition[] readVariableDefs, out List<Instruction> allReadInsts)
        {
            /* It's very important to read everything
             * from the PooledReader before applying any
             * exit logic. Should the method return before
             * reading the data then anything after the rpc
             * packet will be malformed due to invalid index. */
            readVariableDefs = new VariableDefinition[serializedParameters.Count];
            allReadInsts = new();

            //True if last parameter is a connection and a server rpc.
            for (int i = 0; i < serializedParameters.Count; i++)
            {
                //Get read instructions and insert it before the return.
                List<Instruction> insts = GetClass<ReaderProcessor>().CreateRead(methodDef, readerParameterDef, serializedParameters[i].ParameterType, out readVariableDefs[i]);
                allReadInsts.AddRange(insts);
            }
        }

        /// <summary>
        /// Creates conditions that clients must pass to send a ServerRpc.
        /// </summary>
        /// <param name = "createdProcessor"></param>
        /// <param name = "rpcAttribute"></param>
        private void CreateServerRpcConditionsForClient(MethodDefinition methodDef, CustomAttribute rpcAttribute)
        {
            bool requireOwnership = rpcAttribute.GetField(REQUIREOWNERSHIP_NAME, true);
            //If (!base.IsOwner);
            if (requireOwnership)
                GetClass<NetworkBehaviourHelper>().CreateLocalClientIsOwnerCheck(methodDef, LoggingType.Warning, false, false, true);
            //If (!base.IsClient)
            LoggingType loggingType = rpcAttribute.GetField(LOGGING_NAME, LoggingType.Warning);
            GetClass<NetworkBehaviourHelper>().CreateIsClientCheck(methodDef, loggingType, false, true, false);
        }

        /// <summary>
        /// Creates conditions that server must pass to process a ServerRpc.
        /// </summary>
        /// <param name = "createdProcessor"></param>
        /// <param name = "rpcAttribute"></param>
        /// <returns>Ret instruction.</returns>
        private Instruction CreateServerRpcConditionsForServer(ILProcessor createdProcessor, bool requireOwnership, ParameterDefinition connectionParametereDef)
        {
            /* Don't need to check if server on receiving end.
             * Next compare connection with owner. */
            //If (!base.CompareOwner);
            if (requireOwnership)
                return GetClass<NetworkBehaviourHelper>().CreateRemoteClientIsOwnerCheck(createdProcessor, connectionParametereDef);
            else
                return null;
        }

        /// <summary>
        /// Creates conditions that server must pass to process a ClientRpc.
        /// </summary>
        /// <param name = "createdProcessor"></param>
        private void CreateClientRpcConditionsForServer(MethodDefinition methodDef, CustomAttribute rpcAttribute)
        {
            LoggingType loggingType = rpcAttribute.GetField(LOGGING_NAME, LoggingType.Warning);
            //If (!base.IsServer)
            GetClass<NetworkBehaviourHelper>().CreateIsServerCheck(methodDef, loggingType, false, false, false);
        }

        /// <summary>
        /// Creates a method containing the logic which will run when receiving the Rpc.
        /// </summary>
        /// <param name = "originalMd"></param>
        /// <returns></returns>
        private MethodDefinition CreateRpcLogicMethod(List<AttributeData> datas, CreatedRpc cr, out bool intentionallyNull)
        {
            intentionallyNull = false;

            TypeDefinition typeDef = cr.TypeDef;
            MethodDefinition originalMd = cr.OriginalMethodDef;


            //Methodname for logic methods do not use prefixes because there can be only one.
            string methodName = $"{LOGIC_PREFIX}{GetMethodNameAsParameters(originalMd)}";

            /* Check if method exists first. If it does already exist then return found method.
             * This can happen if the logic method was already made when using multiple Rpc attributes
             * such as TargetRpc/ObserversRpc. */
            MethodDefinition createdMd = typeDef.GetMethod(methodName);
            if (createdMd != null)
                return createdMd;

            //If here logic method does not exist yet.
            createdMd = new(methodName, cr.OriginalMethodDef.Attributes, ImportReference(typeof(void)));
            typeDef.Methods.Add(createdMd);
            createdMd.Body.InitLocals = true;

            foreach (ParameterDefinition pd in originalMd.Parameters)
                createdMd.Parameters.Add(new(ImportReference(pd.ParameterType)));

            GetClass<GeneralHelper>().CopyIntoMethod(cr.OriginalMethodDef, createdMd);

            /* This is a partial fix for Unity 2021 IL2CPP builds. The issue appears to be resolved in Unity 2022.
             * In Unity 2021 when calling a generated method in a generic class the codegen must
             * strip the calls to the method of its generics. This is of course improper code but
             * that some reason is the resolution, because Unity. However, even with this fix if the
             * developer makes use of the generic properties of the class from the offending method
             * there is a fair chance the application will crash. */
            #if !UNITY_2022_3_OR_NEWER
            /* If the declaring type has a generic then we need to see if any
             * logic instructions call methods in another or same generic class. */
            ILProcessor processor = createdMd.Body.GetILProcessor();
            List<Instruction> inserter = new();

            //base.LogWarning($"Created {createdMd.Name}. Original {cr.OriginalMethodDef.Name}");
            Collection<Instruction> instructions = createdMd.Body.Instructions;
            for (int i = 0; i < instructions.Count; i++)
            {
                inserter.Clear();

                Instruction v = instructions[i];
                OpCode instrOpCode = v.OpCode;

                if (instrOpCode == OpCodes.Callvirt || instrOpCode == OpCodes.Call)
                {
                    MethodDefinition calledMd = null;

                    if (v.Operand is MethodDefinition md)
                    {
                        calledMd = md;
                    }
                    else if (v.Operand is MethodReference mr)
                    {
                        //If methodReference declaring type has generics.
                        if (mr.DeclaringType.ContainsGenericParameter)
                            calledMd = mr.CachedResolve(Session);
                    }

                    //If need to make a new call then remove old and insert.
                    if (calledMd != null)
                    {
                        instructions.RemoveAt(i);
                        inserter.Add(processor.Create(instrOpCode, calledMd));
                        processor.InsertAt(i, inserter);
                    }
                }
            }
            #endif

            return createdMd;
        }

        /// <summary>
        /// Finds and fixes call to base methods within remote calls
        /// <para>For example, changes `base.CmdDoSomething` to `base.UserCode_CmdDoSomething` within `this.UserCode_CmdDoSomething`</para>
        /// </summary>
        /// <param name = "type"></param>
        /// <param name = "createdMethodDef"></param>
        private void RedirectBaseCall(MethodDefinition createdMethodDef, MethodDefinition originalMethodDef)
        {
            //All logic RPCs end with the logic suffix.
            if (!createdMethodDef.Name.StartsWith(LOGIC_PREFIX))
                return;
            //Not virtual, no need to check.
            if (!createdMethodDef.IsVirtual)
                return;

            foreach (Instruction instruction in createdMethodDef.Body.Instructions)
            {
                // if call to base.RpcDoSomething within this.RpcDoSOmething.
                if (GetClass<GeneralHelper>().IsCallToMethod(instruction, out MethodDefinition calledMethod) && calledMethod.Name == originalMethodDef.Name)
                {
                    MethodReference baseLogicMd = createdMethodDef.DeclaringType.GetMethodDefinitionInAnyBase(Session, createdMethodDef.Name);
                    if (baseLogicMd == null)
                    {
                        LogError($"Could not find base method for {createdMethodDef.Name}.");
                        return;
                    }

                    instruction.Operand = ImportReference(baseLogicMd);
                }
            }
        }

        /// <summary>
        /// Redirects calls from the original Rpc method to the writer method.
        /// </summary>
        private void RedirectOriginalToWriter(List<CreatedRpc> createdRpcs)
        {
            /* If there are multiple attributes/createdRpcs they will
             * share the same originalMd so it's fine to take the first
             * entry. */
            MethodDefinition originalMd = createdRpcs[0].OriginalMethodDef;


            ILProcessor processor = originalMd.Body.GetILProcessor();
            originalMd.Body.Instructions.Clear();

            //If only one rpc type.
            if (createdRpcs.Count == 1)
            {
                processor.Emit(OpCodes.Ldarg_0); //this.
                //Parameters.
                foreach (ParameterDefinition pd in originalMd.Parameters)
                    processor.Emit(OpCodes.Ldarg, pd);

                //Call method.
                MethodReference writerMr = ImportReference(createdRpcs[0].WriterMethodDef);
                processor.Emit(OpCodes.Call, writerMr);

                AddRunLocally(createdRpcs[0]);
            }
            //More than one which means it's an observer/targetRpc combo.
            else
            {
                CreatedRpc observersRpc = createdRpcs.GetCreatedRpc(RpcType.Observers);
                MethodReference observerWriterMr = ImportReference(observersRpc.WriterMethodDef);

                CreatedRpc targetRpc = createdRpcs.GetCreatedRpc(RpcType.Target);
                MethodReference targetWriterMr = ImportReference(targetRpc.WriterMethodDef);

                Instruction targetRpcInst = processor.Create(OpCodes.Nop);
                Instruction afterTargetRpcInst = processor.Create(OpCodes.Nop);
                /* if (targetConn == null)
                 *      WriteObserverRpc
                 * else
                 *      WriteTargetRpc */
                processor.Emit(OpCodes.Ldarg, originalMd.Parameters[0]);
                processor.Emit(OpCodes.Brtrue_S, targetRpcInst);
                //Insert parameters.
                processor.Emit(OpCodes.Ldarg_0);
                foreach (ParameterDefinition pd in originalMd.Parameters)
                    processor.Emit(OpCodes.Ldarg, pd);
                processor.Emit(OpCodes.Call, observerWriterMr);
                AddRunLocally(observersRpc);
                //else (target).
                processor.Emit(OpCodes.Br_S, afterTargetRpcInst);
                processor.Append(targetRpcInst);
                //Insert parameters.
                processor.Emit(OpCodes.Ldarg_0);
                foreach (ParameterDefinition pd in originalMd.Parameters)
                    processor.Emit(OpCodes.Ldarg, pd);
                processor.Emit(OpCodes.Call, targetWriterMr);
                AddRunLocally(targetRpc);
                processor.Append(afterTargetRpcInst);
            }

            //Adds run locally logic if needed.
            void AddRunLocally(CreatedRpc cr)
            {
                //Runlocally.
                if (cr.RunLocally)
                {
                    processor.Emit(OpCodes.Ldarg_0); //this.
                    //Parameters.
                    foreach (ParameterDefinition pd in originalMd.Parameters)
                        processor.Emit(OpCodes.Ldarg, pd);

                    MethodReference logicMr = cr.LogicMethodDef.GetMethodReference(Session);
                    processor.Emit(OpCodes.Call, logicMr);
                }
            }

            processor.Emit(OpCodes.Ret);
        }

        #region CreateSend
        /// <summary>
        /// Creates a call to SendServerRpc on NetworkBehaviour.
        /// </summary>
        /// <param name = "writerVariableDef"></param>
        /// <param name = "channel"></param>
        private List<Instruction> CreateSendServerRpc(MethodDefinition methodDef, uint methodHash, VariableDefinition writerVariableDef, VariableDefinition channelVariableDef, CustomAttribute rpcAttribute)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            insts.AddRange(CreateSendRpcCommon(processor, methodHash, writerVariableDef, channelVariableDef, rpcAttribute));
            //Call NetworkBehaviour.
            insts.Add(processor.Create(OpCodes.Call, GetClass<NetworkBehaviourHelper>().SendServerRpc_MethodRef));

            return insts;
        }

        /// <summary>
        /// Creates a call to SendObserversRpc on NetworkBehaviour.
        /// </summary>
        private List<Instruction> CreateSendObserversRpc(MethodDefinition methodDef, uint methodHash, VariableDefinition writerVariableDef, VariableDefinition channelVariableDef, CustomAttribute rpcAttribute)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            insts.AddRange(CreateSendRpcCommon(processor, methodHash, writerVariableDef, channelVariableDef, rpcAttribute));
            //Also add if buffered.
            bool bufferLast = rpcAttribute.GetField(BUFFERLAST_NAME, false);
            bool excludeOwner = rpcAttribute.GetField(EXCLUDEOWNER_NAME, false);
            bool excludeServer = rpcAttribute.GetField(EXCLUDESERVER_NAME, false);

            //Warn user if any values are byref.
            bool usedByref = false;
            foreach (ParameterDefinition item in methodDef.Parameters)
            {
                if (item.IsIn)
                {
                    usedByref = true;
                    break;
                }
            }
            if (usedByref)
                LogWarning($"Method {methodDef.FullName} takes an argument by reference. While this is supported, using BufferLast in addition to by reference arguements will buffer the value as it was serialized, not as it is when sending buffered.");

            insts.Add(processor.Create(OpCodes.Ldc_I4, bufferLast.ToInt()));
            insts.Add(processor.Create(OpCodes.Ldc_I4, excludeServer.ToInt()));
            insts.Add(processor.Create(OpCodes.Ldc_I4, excludeOwner.ToInt()));
            //Call NetworkBehaviour.
            insts.Add(processor.Create(OpCodes.Call, GetClass<NetworkBehaviourHelper>().SendObserversRpc_MethodRef));

            return insts;
        }

        /// <summary>
        /// Creates a call to SendTargetRpc on NetworkBehaviour.
        /// </summary>
        private List<Instruction> CreateSendTargetRpc(MethodDefinition methodDef, uint methodHash, VariableDefinition writerVariableDef, VariableDefinition channelVariableDef, ParameterDefinition targetConnectionParameterDef, List<AttributeData> attributeDatas)
        {
            List<Instruction> insts = new();
            ILProcessor processor = methodDef.Body.GetILProcessor();

            CustomAttribute rpcAttribute = attributeDatas.GetAttribute(Session, RpcType.Target);
            bool validateTarget = rpcAttribute.GetField(VALIDATETARGET_NAME, true);
            bool excludeServer = rpcAttribute.GetField(EXCLUDESERVER_NAME, false);

            insts.AddRange(CreateSendRpcCommon(processor, methodHash, writerVariableDef, channelVariableDef, rpcAttribute));
            insts.Add(processor.Create(OpCodes.Ldarg, targetConnectionParameterDef));
            insts.Add(processor.Create(OpCodes.Ldc_I4, excludeServer.ToInt()));
            insts.Add(processor.Create(OpCodes.Ldc_I4, validateTarget.ToInt()));
            //Call NetworkBehaviour.
            insts.Add(processor.Create(OpCodes.Call, GetClass<NetworkBehaviourHelper>().SendTargetRpc_MethodRef));

            return insts;
        }

        /// <summary>
        /// Writes common properties that all SendRpc methods use.
        /// </summary>
        private List<Instruction> CreateSendRpcCommon(ILProcessor processor, uint methodHash, VariableDefinition writerVariableDef, VariableDefinition channelVariableDef, CustomAttribute rpcAttribute)
        {
            List<Instruction> insts = new();

            insts.Add(processor.Create(OpCodes.Ldarg_0)); // argument: this
            insts.Add(processor.Create(OpCodes.Ldc_I4, (int)methodHash));
            //reference to PooledWriter.
            insts.Add(processor.Create(OpCodes.Ldloc, writerVariableDef));
            //reference to Channel.
            insts.Add(processor.Create(OpCodes.Ldloc, channelVariableDef));

            int orderType = (int)rpcAttribute.GetField(DATAORDERTYPE_NAME, DataOrderType.Default);
            insts.Add(processor.Create(OpCodes.Ldc_I4, orderType));

            return insts;
        }
        #endregion
    }
}