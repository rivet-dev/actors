using FishNet.Broadcast;
using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.CodeGenerating.Processing;
using FishNet.CodeGenerating.Processing.Rpc;
using FishNet.Configuring;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Unity.CompilationPipeline.Common.ILPostProcessing;

namespace FishNet.CodeGenerating.ILCore
{
    public class FishNetILPP : ILPostProcessor
    {
        #region Const.
        internal const string RUNTIME_ASSEMBLY_NAME = "FishNet.Runtime";
        #endregion

        public override bool WillProcess(ICompiledAssembly compiledAssembly)
        {
            if (compiledAssembly.Name.StartsWith("Unity."))
                return false;
            if (compiledAssembly.Name.StartsWith("UnityEngine."))
                return false;
            if (compiledAssembly.Name.StartsWith("UnityEditor."))
                return false;
            if (compiledAssembly.Name.Contains("Editor"))
                return false;

            /* This line contradicts the one below where referencesFishNet
             * becomes true if the assembly is FishNetAssembly. This is here
             * intentionally to stop codegen from running on the runtime
             * fishnet assembly, but the option below is for debugging. I would
             * comment out this check if I wanted to compile fishnet runtime. */
            // if (CODEGEN_THIS_NAMESPACE.Length == 0)
            // {
            //    if (compiledAssembly.Name == RUNTIME_ASSEMBLY_NAME)
            //        return false;
            // }
            bool referencesFishNet = IsFishNetAssembly(compiledAssembly) || compiledAssembly.References.Any(filePath => Path.GetFileNameWithoutExtension(filePath) == RUNTIME_ASSEMBLY_NAME);
            return referencesFishNet;
        }

        public override ILPostProcessor GetInstance() => this;

        public override ILPostProcessResult Process(ICompiledAssembly compiledAssembly)
        {
            AssemblyDefinition assemblyDef = ILCoreHelper.GetAssemblyDefinition(compiledAssembly);
            if (assemblyDef == null)
                return null;

            // Check WillProcess again; somehow certain editor scripts skip the WillProcess check.
            if (!WillProcess(compiledAssembly))
                return null;

            CodegenSession session = new();
            if (!session.Initialize(assemblyDef.MainModule))
                return null;


            bool modified = false;

            bool fnAssembly = IsFishNetAssembly(compiledAssembly);
            if (fnAssembly)
                modified |= ModifyMakePublicMethods(session);
            /* If one or more scripts use RPCs but don't inherit NetworkBehaviours
             * then don't bother processing the rest. */
            if (session.GetClass<NetworkBehaviourProcessor>().NonNetworkBehaviourHasInvalidAttributes(session.Module.Types))
                return new(null, session.Diagnostics);

            modified |= session.GetClass<WriterProcessor>().Process();
            modified |= session.GetClass<ReaderProcessor>().Process();
            modified |= CreateDeclaredSerializerDelegates(session);
            modified |= CreateDeclaredSerializers(session);
            modified |= CreateDeclaredComparerDelegates(session);
            modified |= CreateIncludeSerializationSerializers(session);
            modified |= CreateIBroadcast(session);
#if !DISABLE_QOL_ATTRIBUTES
            modified |= CreateQOLAttributes(session);
#endif
            modified |= CreateNetworkBehaviours(session);
            modified |= CreateSerializerInitializeDelegates(session);

            if (fnAssembly)
            {
                AssemblyNameReference anr = session.Module.AssemblyReferences.FirstOrDefault<AssemblyNameReference>(x => x.FullName == session.Module.Assembly.FullName);
                if (anr != null)
                    session.Module.AssemblyReferences.Remove(anr);
            }

            /* If there are warnings about SyncVars being in different assemblies.
             * This is awful ... codegen would need to be reworked to save
             * syncvars across all assemblies so that scripts referencing them from
             * another assembly can have it's instructions changed. This however is an immense
             * amount of work so it will have to be put on hold, for... a long.. long while. */
            if (session.DifferentAssemblySyncVars.Count > 0)
            {
                StringBuilder sb = new();
                sb.AppendLine($"Assembly {session.Module.Name} has inherited access to SyncVars in different assemblies. When accessing SyncVars across assemblies be sure to use Get/Set methods withinin the inherited assembly script to change SyncVars. Accessible fields are:");

                foreach (FieldDefinition item in session.DifferentAssemblySyncVars)
                    sb.AppendLine($"Field {item.Name} within {item.DeclaringType.FullName} in assembly {item.Module.Name}.");

                session.LogWarning("v------- IMPORTANT -------v");
                session.LogWarning(sb.ToString());
                session.DifferentAssemblySyncVars.Clear();
            }

            // session.LogWarning($"Assembly {compiledAssembly.Name} took {stopwatch.ElapsedMilliseconds}.");
            if (!modified)
                return null;

            MemoryStream pe = new();
            MemoryStream pdb = new();
            WriterParameters writerParameters = new()
            {
                SymbolWriterProvider = new PortablePdbWriterProvider(),
                SymbolStream = pdb,
                WriteSymbols = true
            };
            assemblyDef.Write(pe, writerParameters);
            return new(new(pe.ToArray(), pdb.ToArray()), session.Diagnostics);
        }

        /// <summary>
        /// Makees methods public scope which use CodegenMakePublic attribute.
        /// </summary>
        /// <returns></returns>
        private bool ModifyMakePublicMethods(CodegenSession session)
        {
            string makePublicTypeFullName = typeof(MakePublicAttribute).FullName;
            foreach (TypeDefinition td in session.Module.Types)
            {
                foreach (CustomAttribute tdCustomAttribute in td.CustomAttributes)
                {
                    if (tdCustomAttribute.AttributeType.FullName == makePublicTypeFullName)
                    {
                        td.Attributes &= ~TypeAttributes.NotPublic;
                        td.Attributes |= TypeAttributes.Public;
                    }
                }
                foreach (MethodDefinition md in td.Methods)
                {
                    foreach (CustomAttribute ca in md.CustomAttributes)
                    {
                        if (ca.AttributeType.FullName == makePublicTypeFullName)
                        {
                            md.Attributes &= ~MethodAttributes.Assembly;
                            md.Attributes |= MethodAttributes.Public;
                        }
                    }
                }
            }

            // There is always at least one modified.
            return true;
        }

        /// <summary>
        /// Creates delegates for user declared serializers.
        /// </summary>
        internal bool CreateDeclaredSerializerDelegates(CodegenSession session)
        {
            bool modified = false;

            List<TypeDefinition> allTypeDefs = session.Module.Types.ToList();
            foreach (TypeDefinition td in allTypeDefs)
            {
                if (session.GetClass<GeneralHelper>().HasExcludeSerializationAttribute(td))
                    continue;

                if (td.Attributes.HasFlag(WriterProcessor.CUSTOM_SERIALIZER_TYPEDEF_ATTRIBUTES))
                    modified |= session.GetClass<CustomSerializerProcessor>().CreateSerializerDelegates(td, true);
            }

            return modified;
        }

        /// <summary>
        /// Creates serializers for custom types within user declared serializers.
        /// </summary>
        private bool CreateDeclaredSerializers(CodegenSession session)
        {
            bool modified = false;

            List<TypeDefinition> allTypeDefs = session.Module.Types.ToList();
            foreach (TypeDefinition td in allTypeDefs)
            {
                if (session.GetClass<GeneralHelper>().HasExcludeSerializationAttribute(td))
                    continue;

                if (td.Attributes.HasFlag(WriterProcessor.CUSTOM_SERIALIZER_TYPEDEF_ATTRIBUTES))
                    modified |= session.GetClass<CustomSerializerProcessor>().CreateSerializers(td);
            }

            return modified;
        }

        /// <summary>
        /// Creates delegates for user declared comparers.
        /// </summary>
        internal bool CreateDeclaredComparerDelegates(CodegenSession session)
        {
            bool modified = false;
            List<TypeDefinition> allTypeDefs = session.Module.Types.ToList();
            foreach (TypeDefinition td in allTypeDefs)
            {
                if (session.GetClass<GeneralHelper>().HasExcludeSerializationAttribute(td))
                    continue;

                modified |= session.GetClass<CustomSerializerProcessor>().CreateComparerDelegates(td);
            }

            return modified;
        }

        /// <summary>
        /// Creates serializers for types that use IncludeSerialization attribute.
        /// </summary>
        private bool CreateIncludeSerializationSerializers(CodegenSession session)
        {
            string attributeName = typeof(IncludeSerializationAttribute).FullName;
            WriterProcessor wp = session.GetClass<WriterProcessor>();
            ReaderProcessor rp = session.GetClass<ReaderProcessor>();

            bool modified = false;
            List<TypeDefinition> allTypeDefs = session.Module.Types.ToList();
            foreach (TypeDefinition td in allTypeDefs)
            {
                if (!CanSerialize())
                    continue;

                TypeReference tr = session.ImportReference(td);
                string traceText = session.TypeDefinitionTraceText(td);

                if (wp.CreateWriter(tr, traceText) != null && rp.CreateReader(tr, traceText) != null)
                    modified = true;
                else
                    session.LogError($"Failed to create serializers for {td.FullName}.");

                bool CanSerialize()
                {
                    foreach (CustomAttribute item in td.CustomAttributes)
                    {
                        if (item.AttributeType.FullName == attributeName)
                            return true;
                    }

                    return false;
                }
            }

            return modified;
        }

        /// <summary>
        /// Creaters serializers and calls for IBroadcast.
        /// </summary>
        /// <param name = "moduleDef"></param>
        /// <param name = "diagnostics"></param>
        private bool CreateIBroadcast(CodegenSession session)
        {
            bool modified = false;

            string networkBehaviourFullName = session.GetClass<NetworkBehaviourHelper>().FullName;
            HashSet<TypeDefinition> typeDefs = new();
            foreach (TypeDefinition td in session.Module.Types)
            {
                TypeDefinition climbTd = td;
                do
                {
                    // Reached NetworkBehaviour class.
                    if (climbTd.FullName == networkBehaviourFullName)
                        break;

                    ///* Check initial class as well all types within
                    // * the class. Then check all of it's base classes. */
                    if (climbTd.ImplementsInterface<IBroadcast>())
                        typeDefs.Add(climbTd);
                    // 7ms

                    // Add nested. Only going to go a single layer deep.
                    foreach (TypeDefinition nestedTypeDef in td.NestedTypes)
                    {
                        if (nestedTypeDef.ImplementsInterface<IBroadcast>())
                            typeDefs.Add(nestedTypeDef);
                    }
                    // 0ms

                    climbTd = climbTd.GetNextBaseTypeDefinition(session);
                    // this + name check 40ms
                } while (climbTd != null);
            }


            // Create reader/writers for found typeDefs.
            foreach (TypeDefinition td in typeDefs)
            {
                TypeReference typeRef = session.ImportReference(td);
                bool canSerialize = session.GetClass<GeneralHelper>().HasSerializerAndDeserializer(typeRef, true);
                if (!canSerialize)
                    session.LogError($"Broadcast {td.Name} does not support serialization. Use a supported type or create a custom serializer.");
                else
                    modified = true;
            }

            return modified;
        }

        /// <summary>
        /// Handles QOLAttributes such as [Server].
        /// </summary>
        /// <returns></returns>
        private bool CreateQOLAttributes(CodegenSession session)
        {
            bool modified = false;

            bool codeStripping = false;
            List<TypeDefinition> allTypeDefs = session.Module.Types.ToList();

            /* First pass, potentially only pass.
             * If code stripping them this will be run again. The first iteration
             * is to ensure things are removed in the proper order. */
            foreach (TypeDefinition td in allTypeDefs)
            {
                if (session.GetClass<GeneralHelper>().HasExcludeSerializationAttribute(td))
                    continue;

                modified |= session.GetClass<QolAttributeProcessor>().Process(td, codeStripping);
            }


            return modified;
        }

        /// <summary>
        /// Creates NetworkBehaviour changes.
        /// </summary>
        private bool CreateNetworkBehaviours(CodegenSession session)
        {
            // Get all network behaviours to process.
            List<TypeDefinition> networkBehaviourTypeDefs = session.Module.Types.Where(td => td.IsSubclassOf(session, session.GetClass<NetworkBehaviourHelper>().FullName)).ToList();

            /* Remove types which are inherited. This gets the child most networkbehaviours.
             * Since processing iterates upward from each child there is no reason
             * to include any inherited NBs. */
            RemoveInheritedTypeDefinitions(networkBehaviourTypeDefs);

            foreach (TypeDefinition typeDef in networkBehaviourTypeDefs)
            {
                session.ImportReference(typeDef);
                session.GetClass<NetworkBehaviourProcessor>().ProcessLocal(typeDef);
            }

            // Call base methods on RPCs.
            foreach (TypeDefinition td in session.Module.Types)
                session.GetClass<RpcProcessor>().RedirectBaseCalls();

            /* Removes typedefinitions which are inherited by
             * another within tds. For example, if the collection
             * td contains A, B, C and our structure is
             * A : B : C then B and C will be removed from the collection
             *  Since they are both inherited by A. */
            void RemoveInheritedTypeDefinitions(List<TypeDefinition> tds)
            {
                HashSet<TypeDefinition> inheritedTds = new();
                /* Remove any networkbehaviour typedefs which are inherited by
                 * another networkbehaviour typedef. */
                for (int i = 0; i < tds.Count; i++)
                {
                    /* Iterates all base types and
                     * adds them to inheritedTds so long
                     * as the base type is not a NetworkBehaviour. */
                    TypeDefinition copyTd = tds[i].GetNextBaseTypeDefinition(session);
                    while (copyTd != null)
                    {
                        // Class is NB.
                        if (copyTd.FullName == session.GetClass<NetworkBehaviourHelper>().FullName)
                            break;

                        inheritedTds.Add(copyTd);
                        copyTd = copyTd.GetNextBaseTypeDefinition(session);
                    }
                }

                // Remove all inherited types.
                foreach (TypeDefinition item in inheritedTds)
                    tds.Remove(item);
            }

            // Moment a NetworkBehaviour exist the assembly is considered modified.
            bool modified = networkBehaviourTypeDefs.Count > 0;
            return modified;
        }

        /// <summary>
        /// Creates generic delegates for all read and write methods.
        /// </summary>
        /// <param name = "moduleDef"></param>
        /// <param name = "diagnostics"></param>
        private bool CreateSerializerInitializeDelegates(CodegenSession session)
        {
            session.GetClass<WriterProcessor>().CreateInitializeDelegates();
            session.GetClass<ReaderProcessor>().CreateInitializeDelegates();

            return true;
        }

        internal static bool IsFishNetAssembly(ICompiledAssembly assembly) => assembly.Name == RUNTIME_ASSEMBLY_NAME;
        internal static bool IsFishNetAssembly(CodegenSession session) => session.Module.Assembly.Name.Name == RUNTIME_ASSEMBLY_NAME;
        internal static bool IsFishNetAssembly(ModuleDefinition moduleDef) => moduleDef.Assembly.Name.Name == RUNTIME_ASSEMBLY_NAME;
    }
}