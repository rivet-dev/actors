using FishNet.CodeGenerating.Helping;
using FishNet.CodeGenerating.Helping.Extension;
using MonoFN.Cecil;
using MonoFN.Cecil.Cil;
using System.Collections.Generic;

namespace FishNet.CodeGenerating.Extension
{
    internal static class MethodDefinitionExtensions
    {
        public const MethodAttributes PUBLIC_VIRTUAL_ATTRIBUTES = MethodAttributes.Public | MethodAttributes.Virtual | MethodAttributes.HideBySig;
        public const MethodAttributes PROTECTED_VIRTUAL_ATTRIBUTES = MethodAttributes.Family | MethodAttributes.Virtual | MethodAttributes.HideBySig;

        /// <summary>
        /// Returns a custom attribute.
        /// </summary>
        public static CustomAttribute GetCustomAttribute(this MethodDefinition md, string attributeFullName)
        {
            if (md == null)
                return null;

            foreach (CustomAttribute item in md.CustomAttributes)
            {
                if (item.AttributeType.FullName == attributeFullName)
                    return item;
            }

            // Not found.
            return null;
        }

        /// <summary>
        /// Clears the method content and returns ret.
        /// </summary>
        internal static void ClearMethodWithRet(this MethodDefinition md, CodegenSession session, ModuleDefinition importReturnModule = null)
        {
            md.Body.Instructions.Clear();
            ILProcessor processor = md.Body.GetILProcessor();
            processor.Add(session.GetClass<GeneralHelper>().CreateRetDefault(md, importReturnModule));
        }

        /// <summary>
        /// Returns the ParameterDefinition index from end of parameters.
        /// </summary>
        /// <param name = "md"></param>
        /// <param name = "index"></param>
        /// <returns></returns>
        internal static ParameterDefinition GetEndParameter(this MethodDefinition md, int index)
        {
            // Not enough parameters.
            if (md.Parameters.Count < index + 1)
                return null;

            return md.Parameters[md.Parameters.Count - (index + 1)];
        }

        /// <summary>
        /// Creates a variable type within the body and returns it's VariableDef.
        /// </summary>
        internal static VariableDefinition CreateVariable(this MethodDefinition methodDef, TypeReference variableTypeRef)
        {
            VariableDefinition variableDef = new(variableTypeRef);
            methodDef.Body.Variables.Add(variableDef);
            return variableDef;
        }

        /// <summary>
        /// Creates a variable type within the body and returns it's VariableDef.
        /// </summary>
        internal static VariableDefinition CreateVariable(this MethodDefinition methodDef, CodegenSession session, System.Type variableType)
        {
            return CreateVariable(methodDef, session.GetClass<GeneralHelper>().GetTypeReference(variableType));
        }

        /// <summary>
        /// Returns the proper OpCode to use for call methods.
        /// </summary>
        public static OpCode GetCallOpCode(this MethodDefinition md)
        {
            if (md.Attributes.HasFlag(MethodAttributes.Virtual))
                return OpCodes.Callvirt;
            else
                return OpCodes.Call;
        }

        /// <summary>
        /// Returns the proper OpCode to use for call methods.
        /// </summary>
        public static OpCode GetCallOpCode(this MethodReference mr, CodegenSession session)
        {
            return mr.CachedResolve(session).GetCallOpCode();
        }

        /// <summary>
        /// Adds a parameter and returns added parameters.
        /// </summary>
        public static ParameterDefinition CreateParameter(this MethodDefinition thisMd, CodegenSession session, ParameterAttributes attr, System.Type type)
        {
            TypeReference parameterTypeRef = session.ImportReference(type);
            ParameterDefinition pd = new($"p{thisMd.Parameters.Count}", attr, parameterTypeRef);
            thisMd.Parameters.Add(pd);
            return pd;
        }

        /// <summary>
        /// Adds otherMd parameters to thisMd and returns added parameters.
        /// </summary>
        public static List<ParameterDefinition> CreateParameters(this MethodDefinition thisMd, CodegenSession session, MethodDefinition otherMd)
        {
            List<ParameterDefinition> results = new();

            foreach (ParameterDefinition pd in otherMd.Parameters)
            {
                int currentCount = thisMd.Parameters.Count;
                string name = pd.Name + currentCount;
                ParameterDefinition parameterDef = pd.CloneImported(session, thisMd, name);

                if (parameterDef == null || thisMd.Parameters == null)
                {
                    session.LogError($"ParameterDefinition or collection is null. Definition null: {parameterDef == null}. Collection null: {thisMd.Parameters == null}.");
                }
                else
                {
                    thisMd.Parameters.Add(parameterDef);
                    results.Add(parameterDef);
                }
            }

            return results;
        }

        /// <summary>
        /// Returns a method reference while considering if declaring type is generic.
        /// </summary>
        public static MethodReference GetMethodReference(this MethodDefinition md, CodegenSession session)
        {
            MethodReference methodRef = session.ImportReference(md);

            // Is generic.
            if (md.DeclaringType.HasGenericParameters)
            {
                GenericInstanceType git = methodRef.DeclaringType.MakeGenericInstanceType();
                MethodReference result = new(md.Name, md.ReturnType)
                {
                    HasThis = md.HasThis,
                    ExplicitThis = md.ExplicitThis,
                    DeclaringType = git,
                    CallingConvention = md.CallingConvention
                };

                foreach (ParameterDefinition pd in md.Parameters)
                    result.Parameters.Add(pd.CloneImported(session, result));

                return result;
            }

            return methodRef;
        }

        /// <summary>
        /// Returns a method reference for a generic method.
        /// </summary>
        public static MethodReference GetMethodReference(this MethodDefinition md, CodegenSession session, TypeReference typeReference)
        {
            MethodReference methodRef = session.ImportReference(md);
            return methodRef.GetMethodReference(session, typeReference);
        }

        /// <summary>
        /// Removes ret if it exist at the end of the method. Returns if ret was removed.
        /// </summary>
        internal static bool RemoveEndRet(this MethodDefinition md, CodegenSession session)
        {
            int count = md.Body.Instructions.Count;
            if (count > 0 && md.Body.Instructions[count - 1].OpCode == OpCodes.Ret)
            {
                md.Body.Instructions.RemoveAt(count - 1);
                return true;
            }
            else
            {
                return false;
            }
        }

        /// <summary>
        /// Returns a method reference for a generic method.
        /// </summary>
        public static MethodReference GetMethodReference(this MethodDefinition md, CodegenSession session, TypeReference[] typeReferences)
        {
            MethodReference methodRef = session.ImportReference(md);
            return methodRef.GetMethodReference(session, typeReferences);
        }

        public static MethodDefinition CreateCopy(this MethodDefinition copiedMd, CodegenSession session, string nameOverride = null, MethodAttributes? attributesOverride = null)
        {
            TypeReference returnType = session.ImportReference(copiedMd.ReturnType);

            MethodAttributes attr = attributesOverride.HasValue ? attributesOverride.Value : copiedMd.Attributes;
            string name = nameOverride == null ? copiedMd.Name : nameOverride;
            MethodDefinition result = new(name, attr, returnType);
            foreach (GenericParameter item in copiedMd.GenericParameters)
                result.GenericParameters.Add(new(item.Name, result) { Attributes = item.Attributes });

            result.CreateParameters(session, copiedMd);
            return result;
        }

        /// <summary>
        /// Makes a method definition public.
        /// </summary>
        public static void SetPublicAttributes(this MethodDefinition md)
        {
            md.Attributes = PUBLIC_VIRTUAL_ATTRIBUTES;
        }

        public static void SetProtectedAttributes(this MethodDefinition md)
        {
            md.Attributes = PROTECTED_VIRTUAL_ATTRIBUTES;
        }
    }
}