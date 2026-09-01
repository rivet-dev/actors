using FishNet.CodeGenerating.Extension;
using MonoFN.Cecil;
using MonoFN.Cecil.Rocks;
using System;

namespace FishNet.CodeGenerating.Helping.Extension
{
    internal static class MethodReferenceExtensions
    {
        /// <summary>
        /// Returns a custom attribute.
        /// </summary>
        public static CustomAttribute GetCustomAttribute(this MethodReference mr, string attributeFullName)
        {
            MethodDefinition md = mr.Resolve();
            return MethodDefinitionExtensions.GetCustomAttribute(md, attributeFullName);
        }

        /// <summary>
        /// Makes a generic method with specified arguments.
        /// </summary>
        /// <param name = "method"></param>
        /// <param name = "genericArguments"></param>
        /// <returns></returns>
        public static GenericInstanceMethod MakeGenericMethod(this MethodReference method, params TypeReference[] genericArguments)
        {
            GenericInstanceMethod result = new(method);
            foreach (TypeReference argument in genericArguments)
                result.GenericArguments.Add(argument);
            return result;
        }

        /// <summary>
        /// Makes a generic method with the same arguments as the original.
        /// </summary>
        /// <param name = "method"></param>
        /// <returns></returns>
        public static GenericInstanceMethod MakeGenericMethod(this MethodReference method)
        {
            GenericInstanceMethod result = new(method);
            foreach (ParameterDefinition pd in method.Parameters)
                result.GenericArguments.Add(pd.ParameterType);

            return result;
        }

        /// <summary>
        /// Returns a method reference for a generic method.
        /// </summary>
        public static MethodReference GetMethodReference(this MethodReference mr, CodegenSession session, TypeReference typeReference)
        {
            return mr.GetMethodReference(session, new TypeReference[] { typeReference });
        }

        /// <summary>
        /// Returns a method reference for a generic method.
        /// </summary>
        public static MethodReference GetMethodReference(this MethodReference mr, CodegenSession session, TypeReference[] typeReferences)
        {
            if (mr.HasGenericParameters)
            {
                if (typeReferences == null || typeReferences.Length == 0)
                {
                    session.LogError($"Method {mr.Name} has generic parameters but TypeReferences are null or 0 length.");
                    return null;
                }
                else
                {
                    GenericInstanceMethod gim = mr.MakeGenericMethod(typeReferences);
                    return gim;
                }
            }
            else
            {
                return mr;
            }
        }

        /// <summary>
        /// Gets a Resolve favoring cached results first.
        /// </summary>
        internal static MethodDefinition CachedResolve(this MethodReference methodRef, CodegenSession session)
        {
            return session.GetClass<GeneralHelper>().GetMethodReferenceResolve(methodRef);
        }

        /// <summary>
        /// Removes ret if it exist at the end of the method. Returns if ret was removed.
        /// </summary>
        internal static bool RemoveEndRet(this MethodReference mr, CodegenSession session)
        {
            MethodDefinition md = mr.CachedResolve(session);
            return MethodDefinitionExtensions.RemoveEndRet(md, session);
        }

        /// <summary>
        /// Given a method of a generic class such as ArraySegment`T.get_Count,
        /// and a generic instance such as ArraySegment`int
        /// Creates a reference to the specialized method  ArraySegment`int`.get_Count
        /// <para> Note that calling ArraySegment`T.get_Count directly gives an invalid IL error </para>
        /// </summary>
        /// <param name = "self"></param>
        /// <param name = "instanceType"></param>
        /// <returns></returns>
        public static MethodReference MakeHostInstanceGeneric(this MethodReference self, CodegenSession session, GenericInstanceType instanceType)
        {
            MethodReference reference = new(self.Name, self.ReturnType, instanceType)
            {
                CallingConvention = self.CallingConvention,
                HasThis = self.HasThis,
                ExplicitThis = self.ExplicitThis
            };

            foreach (ParameterDefinition parameter in self.Parameters)
                reference.Parameters.Add(new(parameter.ParameterType));

            foreach (GenericParameter generic_parameter in self.GenericParameters)
                reference.GenericParameters.Add(new(generic_parameter.Name, reference));

            return session.ImportReference(reference);
        }

        /// <summary>
        /// Given a method of a generic class such as ArraySegment`T.get_Count,
        /// and a generic instance such as ArraySegment`int
        /// Creates a reference to the specialized method  ArraySegment`int`.get_Count
        /// <para> Note that calling ArraySegment`T.get_Count directly gives an invalid IL error </para>
        /// </summary>
        /// <param name = "self"></param>
        /// <param name = "instanceType"></param>
        /// <returns></returns>
        public static MethodReference MakeHostInstanceGeneric(this MethodReference self, TypeReference typeRef, params TypeReference[] args)
        {
            GenericInstanceType git = typeRef.MakeGenericInstanceType(args);
            MethodReference reference = new(self.Name, self.ReturnType, git)
            {
                CallingConvention = self.CallingConvention,
                HasThis = self.HasThis,
                ExplicitThis = self.ExplicitThis
            };

            foreach (ParameterDefinition parameter in self.Parameters)
                reference.Parameters.Add(new(parameter.ParameterType));

            foreach (GenericParameter generic_parameter in self.GenericParameters)
                reference.GenericParameters.Add(new(generic_parameter.Name, reference));

            return reference;
        }

        public static bool Is<T>(this MethodReference method, string name)
        {
            return method.DeclaringType.Is<T>() && method.Name == name;
        }

        public static bool Is<T>(this TypeReference td)
        {
            return Is(td, typeof(T));
        }

        public static bool Is(this TypeReference td, Type t)
        {
            if (t.IsGenericType)
            {
                return td.GetElementType().FullName == t.FullName;
            }
            return td.FullName == t.FullName;
        }
    }
}