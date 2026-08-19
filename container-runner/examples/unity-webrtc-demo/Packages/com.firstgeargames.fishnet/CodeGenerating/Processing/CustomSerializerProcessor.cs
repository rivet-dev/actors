using FishNet.CodeGenerating.Extension;
using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using FishNet.Serializing;
using FishNet.Serializing.Helping;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.Collections.Generic;
using UnityEngine;

namespace FishNet.CodeGenerating.Processing
{
    internal class CustomSerializerProcessor : CodegenBase
    {
        #region Types.
        internal enum ExtensionType
        {
            None,
            Write,
            Read
        }
        #endregion

        internal bool CreateSerializerDelegates(TypeDefinition typeDef, bool replace)
        {
            bool modified = false;
            /* Find all declared methods and register delegates to them.
             * After they are all registered create any custom writers
             * needed to complete the declared methods. It's important to
             * make generated writers after so that a generated method
             * isn't made for a type when the user has already made a declared one. */
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                ExtensionType extensionType = GetExtensionType(methodDef);
                if (extensionType == ExtensionType.None)
                    continue;
                if (GetClass<GeneralHelper>().HasNotSerializableAttribute(methodDef))
                    continue;

                MethodReference methodRef = ImportReference(methodDef);
                if (extensionType == ExtensionType.Write)
                {
                    GetClass<WriterProcessor>().AddWriterMethod(methodRef.Parameters[1].ParameterType, methodRef, false, !replace);
                    modified = true;
                }
                else if (extensionType == ExtensionType.Read)
                {
                    GetClass<ReaderProcessor>().AddReaderMethod(methodRef.ReturnType, methodRef, false, !replace);
                    modified = true;
                }
            }

            return modified;
        }

        /// <summary>
        /// Creates serializers for any custom types for declared methods.
        /// </summary>
        /// <param name = "declaredMethods"></param>
        /// <param name = "moduleDef"></param>
        internal bool CreateSerializers(TypeDefinition typeDef)
        {
            bool modified = false;

            List<(MethodDefinition, ExtensionType)> declaredMethods = new();
            /* Go through all custom serializers again and see if
             * they use any types that the user didn't make a serializer for
             * and that there isn't a built-in type for. Create serializers
             * for these types. */
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                ExtensionType extensionType = GetExtensionType(methodDef);
                if (extensionType == ExtensionType.None)
                    continue;
                if (GetClass<GeneralHelper>().HasNotSerializableAttribute(methodDef))
                    continue;

                declaredMethods.Add((methodDef, extensionType));
                modified = true;
            }
            // Now that all declared are loaded see if any of them need generated serializers.
            foreach ((MethodDefinition methodDef, ExtensionType extensionType) in declaredMethods)
                CreateSerializers(extensionType, methodDef);

            return modified;
        }

        /// <summary>
        /// Creates a custom serializer for any types not handled within users declared.
        /// </summary>
        /// <param name = "extensionType"></param>
        /// <param name = "moduleDef"></param>
        /// <param name = "methodDef"></param>
        /// <param name = "diagnostics"></param>
        private void CreateSerializers(ExtensionType extensionType, MethodDefinition methodDef)
        {
            for (int i = 0; i < methodDef.Body.Instructions.Count; i++)
                CheckToModifyInstructions(extensionType, methodDef, ref i);
        }

        /// <summary>
        /// Creates delegates for custom comparers.
        /// </summary>
        internal bool CreateComparerDelegates(TypeDefinition typeDef)
        {
            bool modified = false;
            GeneralHelper gh = GetClass<GeneralHelper>();
            /* Find all declared methods and register delegates to them.
             * After they are all registered create any custom writers
             * needed to complete the declared methods. It's important to
             * make generated writers after so that a generated method
             * isn't made for a type when the user has already made a declared one. */
            foreach (MethodDefinition methodDef in typeDef.Methods)
            {
                if (gh.HasNotSerializableAttribute(methodDef))
                    continue;
                if (!methodDef.HasCustomAttribute<CustomComparerAttribute>())
                    continue;
                // Validate return type.
                if (methodDef.ReturnType.FullName != gh.GetTypeReference(typeof(bool)).FullName)
                {
                    LogError($"Comparer method {methodDef.Name} in type {typeDef.FullName} must return bool.");
                    continue;
                }
                /* Make sure parameters are correct. */
                // Invalid count.
                if (methodDef.Parameters.Count != 2)
                {
                    LogError($"Comparer method {methodDef.Name} in type {typeDef.FullName} must have exactly two parameters, each of the same type which is being compared.");
                    continue;
                }
                TypeReference p0Tr = methodDef.Parameters[0].ParameterType;
                TypeReference p1Tr = methodDef.Parameters[0].ParameterType;
                // Not the same types.
                if (p0Tr != p1Tr)
                {
                    LogError($"Both parameters must be the same type in comparer method {methodDef.Name} in type {typeDef.FullName}.");
                    continue;
                }

                ImportReference(methodDef);
                ImportReference(p0Tr);
                gh.RegisterComparerDelegate(methodDef, p0Tr);
                gh.CreateComparerDelegate(methodDef, p0Tr);
            }

            return modified;
        }

        /// <summary>
        /// Checks if instructions need to be modified and does so.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <param name = "instructionIndex"></param>
        private void CheckToModifyInstructions(ExtensionType extensionType, MethodDefinition methodDef, ref int instructionIndex)
        {
            Instruction instruction = methodDef.Body.Instructions[instructionIndex];
            // Fields.
            if (instruction.OpCode == OpCodes.Ldsfld || instruction.OpCode == OpCodes.Ldfld)
                CheckFieldReferenceInstruction(extensionType, methodDef, ref instructionIndex);
            // Method calls.
            else if (instruction.OpCode == OpCodes.Call || instruction.OpCode == OpCodes.Callvirt)
                CheckCallInstruction(extensionType, methodDef, ref instructionIndex, (MethodReference)instruction.Operand);
        }

        /// <summary>
        /// Checks if a reader or writer must be generated for a field type.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <param name = "instructionIndex"></param>
        private void CheckFieldReferenceInstruction(ExtensionType extensionType, MethodDefinition methodDef, ref int instructionIndex)
        {
            Instruction instruction = methodDef.Body.Instructions[instructionIndex];
            FieldReference field = (FieldReference)instruction.Operand;
            TypeReference typeRef = field.DeclaringType;

            if (typeRef.IsType(typeof(GenericWriter<>)) || (typeRef.IsType(typeof(GenericReader<>)) && typeRef.IsGenericInstance))
            {
                GenericInstanceType typeGenericInst = (GenericInstanceType)typeRef;
                TypeReference parameterType = typeGenericInst.GenericArguments[0];
                CreateReaderOrWriter(extensionType, methodDef, ref instructionIndex, parameterType);
            }
        }

        /// <summary>
        /// Checks if a reader or writer must be generated for a call type.
        /// </summary>
        /// <param name = "extensionType"></param>
        /// <param name = "moduleDef"></param>
        /// <param name = "methodDef"></param>
        /// <param name = "instructionIndex"></param>
        /// <param name = "method"></param>
        private void CheckCallInstruction(ExtensionType extensionType, MethodDefinition methodDef, ref int instructionIndex, MethodReference method)
        {
            if (!method.IsGenericInstance)
                return;

            // True if call is to read/write.
            bool canCreate = method.Is<Writer>(nameof(Writer.Write)) || method.Is<Reader>(nameof(Reader.Read));

            if (canCreate)
            {
                GenericInstanceMethod instanceMethod = (GenericInstanceMethod)method;
                TypeReference parameterType = instanceMethod.GenericArguments[0];
                if (parameterType.IsGenericParameter)
                    return;

                CreateReaderOrWriter(extensionType, methodDef, ref instructionIndex, parameterType);
            }
        }

        /// <summary>
        /// Creates a reader or writer for parameterType if needed. Otherwise calls existing reader.
        /// </summary>
        private void CreateReaderOrWriter(ExtensionType extensionType, MethodDefinition methodDef, ref int instructionIndex, TypeReference parameterType)
        {
            string traceText = MethodDefinitionTraceText(methodDef);

            ReaderProcessor rp = GetClass<ReaderProcessor>();
            WriterProcessor wp = GetClass<WriterProcessor>();
            ////If parameterType has user declared do nothing.
            // if (wp.IsGlobalSerializer(parameterType))
            //    return;

            if (!parameterType.IsGenericParameter && parameterType.CanBeResolved(Session))
            {
                TypeDefinition typeDefinition = parameterType.CachedResolve(Session);
                // If class and not value type check for accessible constructor.
                if (typeDefinition.IsClass && !typeDefinition.IsValueType)
                {
                    MethodDefinition constructor = typeDefinition.GetDefaultConstructor(Session);
                    // Constructor is inaccessible, cannot create serializer for type.
                    if (constructor != null && !constructor.IsPublic)
                    {
                        LogError($"Unable to generator serializers for {typeDefinition.FullName} because it's constructor is not public.");
                        return;
                    }
                }

                ILProcessor processor = methodDef.Body.GetILProcessor();

                // Find already existing read or write method.
                MethodReference createdMethodRef = extensionType == ExtensionType.Write ? wp.GetWriteMethodReference(parameterType) : rp.GetReadMethodReference(parameterType);

                // If a created method already exist nothing further is required.
                if (createdMethodRef != null)
                {
                    // Replace call to generic with already made serializer.
                    Instruction newInstruction = processor.Create(OpCodes.Call, createdMethodRef);
                    methodDef.Body.Instructions[instructionIndex] = newInstruction;
                    return;
                }

                createdMethodRef = extensionType == ExtensionType.Write ? wp.CreateWriter(parameterType, traceText) : rp.CreateReader(parameterType, traceText);

                // If method was created.
                if (createdMethodRef != null)
                {
                    // Set new instruction.
                    Instruction newInstruction = processor.Create(OpCodes.Call, createdMethodRef);
                    methodDef.Body.Instructions[instructionIndex] = newInstruction;
                }
            }
        }

        /// <summary>
        /// Returns the RPC attribute on a method, if one exist. Otherwise returns null.
        /// </summary>
        /// <param name = "methodDef"></param>
        /// <returns></returns>
        private ExtensionType GetExtensionType(MethodDefinition methodDef)
        {
            bool hasExtensionAttribute = methodDef.HasCustomAttribute<System.Runtime.CompilerServices.ExtensionAttribute>();
            if (!hasExtensionAttribute)
                return ExtensionType.None;

            bool write = methodDef.ReturnType == methodDef.Module.TypeSystem.Void;

            // Return None for Mirror types.
#if MIRROR
            if (write)
            {
                if (methodDef.Parameters.Count > 0 && methodDef.Parameters[0].ParameterType.FullName == "Mirror.NetworkWriter")
                    return ExtensionType.None;
            }
            else
            {
                if (methodDef.Parameters.Count > 0 && methodDef.Parameters[0].ParameterType.FullName == "Mirror.NetworkReader")
                    return ExtensionType.None;
            }
#endif


            string prefix = write ? WriterProcessor.CUSTOM_WRITER_PREFIX : ReaderProcessor.CUSTOM_READER_PREFIX;

            // Does not contain prefix.
            if (methodDef.Name.Length < prefix.Length || methodDef.Name.Substring(0, prefix.Length) != prefix)
                return ExtensionType.None;

            // Make sure first parameter is right.
            if (methodDef.Parameters.Count >= 1)
            {
                TypeReference tr = methodDef.Parameters[0].ParameterType;
                if (tr.FullName != GetClass<WriterImports>().Writer_TypeRef.FullName && tr.FullName != GetClass<ReaderImports>().Reader_TypeRef.FullName)
                    return ExtensionType.None;
            }

            if (write && methodDef.Parameters.Count < 2)
            {
                LogError($"{methodDef.FullName} must have at least two parameters, the first being PooledWriter, and second value to write.");
                return ExtensionType.None;
            }
            else if (!write && methodDef.Parameters.Count < 1)
            {
                LogError($"{methodDef.FullName} must have at least one parameters, the first being PooledReader.");
                return ExtensionType.None;
            }

            return write ? ExtensionType.Write : ExtensionType.Read;
        }
    }
}