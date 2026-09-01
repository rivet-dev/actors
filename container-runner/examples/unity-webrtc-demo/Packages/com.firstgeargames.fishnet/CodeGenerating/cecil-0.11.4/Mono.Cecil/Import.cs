//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using MonoFN.Cecil.Metadata;
using MonoFN.Collections.Generic;
using System;
using System.Collections.Generic;
using SR = System.Reflection;

namespace MonoFN.Cecil
{
    public interface IMetadataImporterProvider
    {
        IMetadataImporter GetMetadataImporter(ModuleDefinition module);
    }

    public interface IMetadataImporter
    {
        AssemblyNameReference ImportReference(AssemblyNameReference reference);
        TypeReference ImportReference(TypeReference type, IGenericParameterProvider context);
        FieldReference ImportReference(FieldReference field, IGenericParameterProvider context);
        MethodReference ImportReference(MethodReference method, IGenericParameterProvider context);
    }

    public interface IReflectionImporterProvider
    {
        IReflectionImporter GetReflectionImporter(ModuleDefinition module);
    }

    public interface IReflectionImporter
    {
        AssemblyNameReference ImportReference(SR.AssemblyName reference);
        TypeReference ImportReference(Type type, IGenericParameterProvider context);
        FieldReference ImportReference(SR.FieldInfo field, IGenericParameterProvider context);
        MethodReference ImportReference(SR.MethodBase method, IGenericParameterProvider context);
    }

    internal struct ImportGenericContext
    {
        private Collection<IGenericParameterProvider> stack;
        public bool IsEmpty
        {
            get { return stack == null; }
        }

        public ImportGenericContext(IGenericParameterProvider provider)
        {
            if (provider == null)
                throw new ArgumentNullException("provider");

            stack = null;

            Push(provider);
        }

        public void Push(IGenericParameterProvider provider)
        {
            if (stack == null)
                stack = new(1) { provider };
            else
                stack.Add(provider);
        }

        public void Pop()
        {
            stack.RemoveAt(stack.Count - 1);
        }

        public TypeReference MethodParameter(string method, int position)
        {
            for (int i = stack.Count - 1; i >= 0; i--)
            {
                MethodReference candidate = stack[i] as MethodReference;
                if (candidate == null)
                    continue;

                if (method != NormalizeMethodName(candidate))
                    continue;

                return candidate.GenericParameters[position];
            }

            throw new InvalidOperationException();
        }

        public string NormalizeMethodName(MethodReference method)
        {
            return method.DeclaringType.GetElementType().FullName + "." + method.Name;
        }

        public TypeReference TypeParameter(string type, int position)
        {
            for (int i = stack.Count - 1; i >= 0; i--)
            {
                TypeReference candidate = GenericTypeFor(stack[i]);

                if (candidate.FullName != type)
                    continue;

                return candidate.GenericParameters[position];
            }

            throw new InvalidOperationException();
        }

        private static TypeReference GenericTypeFor(IGenericParameterProvider context)
        {
            TypeReference type = context as TypeReference;
            if (type != null)
                return type.GetElementType();

            MethodReference method = context as MethodReference;
            if (method != null)
                return method.DeclaringType.GetElementType();

            throw new InvalidOperationException();
        }

        public static ImportGenericContext For(IGenericParameterProvider context)
        {
            return context != null ? new(context) : default(ImportGenericContext);
        }
    }

    public class DefaultReflectionImporter : IReflectionImporter
    {
        protected readonly ModuleDefinition module;

        public DefaultReflectionImporter(ModuleDefinition module)
        {
            Mixin.CheckModule(module);
            this.module = module;
        }

        private enum ImportGenericKind
        {
            Definition,
            Open
        }

        private static readonly Dictionary<Type, ElementType> type_etype_mapping = new(18)
        {
            { typeof(void), ElementType.Void },
            { typeof(bool), ElementType.Boolean },
            { typeof(char), ElementType.Char },
            { typeof(sbyte), ElementType.I1 },
            { typeof(byte), ElementType.U1 },
            { typeof(short), ElementType.I2 },
            { typeof(ushort), ElementType.U2 },
            { typeof(int), ElementType.I4 },
            { typeof(uint), ElementType.U4 },
            { typeof(long), ElementType.I8 },
            { typeof(ulong), ElementType.U8 },
            { typeof(float), ElementType.R4 },
            { typeof(double), ElementType.R8 },
            { typeof(string), ElementType.String },
            { typeof(TypedReference), ElementType.TypedByRef },
            { typeof(IntPtr), ElementType.I },
            { typeof(UIntPtr), ElementType.U },
            { typeof(object), ElementType.Object }
        };

        private TypeReference ImportType(Type type, ImportGenericContext context)
        {
            return ImportType(type, context, ImportGenericKind.Open);
        }

        private TypeReference ImportType(Type type, ImportGenericContext context, ImportGenericKind import_kind)
        {
            if (IsTypeSpecification(type) || ImportOpenGenericType(type, import_kind))
                return ImportTypeSpecification(type, context);

            TypeReference reference = new(string.Empty, type.Name, module, ImportScope(type), type.IsValueType);

            reference.etype = ImportElementType(type);

            if (IsNestedType(type))
                reference.DeclaringType = ImportType(type.DeclaringType, context, import_kind);
            else
                reference.Namespace = type.Namespace ?? string.Empty;

            if (type.IsGenericType)
                ImportGenericParameters(reference, type.GetGenericArguments());

            return reference;
        }

        protected virtual IMetadataScope ImportScope(Type type)
        {
            return ImportScope(type.Assembly);
        }

        private static bool ImportOpenGenericType(Type type, ImportGenericKind import_kind)
        {
            return type.IsGenericType && type.IsGenericTypeDefinition && import_kind == ImportGenericKind.Open;
        }

        private static bool ImportOpenGenericMethod(SR.MethodBase method, ImportGenericKind import_kind)
        {
            return method.IsGenericMethod && method.IsGenericMethodDefinition && import_kind == ImportGenericKind.Open;
        }

        private static bool IsNestedType(Type type)
        {
            return type.IsNested;
        }

        private TypeReference ImportTypeSpecification(Type type, ImportGenericContext context)
        {
            if (type.IsByRef)
                return new ByReferenceType(ImportType(type.GetElementType(), context));

            if (type.IsPointer)
                return new PointerType(ImportType(type.GetElementType(), context));

            if (type.IsArray)
                return new ArrayType(ImportType(type.GetElementType(), context), type.GetArrayRank());

            if (type.IsGenericType)
                return ImportGenericInstance(type, context);

            if (type.IsGenericParameter)
                return ImportGenericParameter(type, context);

            throw new NotSupportedException(type.FullName);
        }

        private static TypeReference ImportGenericParameter(Type type, ImportGenericContext context)
        {
            if (context.IsEmpty)
                throw new InvalidOperationException();

            if (type.DeclaringMethod != null)
                return context.MethodParameter(NormalizeMethodName(type.DeclaringMethod), type.GenericParameterPosition);

            if (type.DeclaringType != null)
                return context.TypeParameter(NormalizeTypeFullName(type.DeclaringType), type.GenericParameterPosition);

            throw new InvalidOperationException();
        }

        private static string NormalizeMethodName(SR.MethodBase method)
        {
            return NormalizeTypeFullName(method.DeclaringType) + "." + method.Name;
        }

        private static string NormalizeTypeFullName(Type type)
        {
            if (IsNestedType(type))
                return NormalizeTypeFullName(type.DeclaringType) + "/" + type.Name;

            return type.FullName;
        }

        private TypeReference ImportGenericInstance(Type type, ImportGenericContext context)
        {
            TypeReference element_type = ImportType(type.GetGenericTypeDefinition(), context, ImportGenericKind.Definition);
            Type[] arguments = type.GetGenericArguments();
            GenericInstanceType instance = new(element_type, arguments.Length);
            Collection<TypeReference> instance_arguments = instance.GenericArguments;

            context.Push(element_type);
            try
            {
                for (int i = 0; i < arguments.Length; i++)
                    instance_arguments.Add(ImportType(arguments[i], context));

                return instance;
            }
            finally
            {
                context.Pop();
            }
        }

        private static bool IsTypeSpecification(Type type)
        {
            return type.HasElementType || IsGenericInstance(type) || type.IsGenericParameter;
        }

        private static bool IsGenericInstance(Type type)
        {
            return type.IsGenericType && !type.IsGenericTypeDefinition;
        }

        private static ElementType ImportElementType(Type type)
        {
            ElementType etype;
            if (!type_etype_mapping.TryGetValue(type, out etype))
                return ElementType.None;

            return etype;
        }

        protected AssemblyNameReference ImportScope(SR.Assembly assembly)
        {
            return ImportReference(assembly.GetName());
        }

        public virtual AssemblyNameReference ImportReference(SR.AssemblyName name)
        {
            Mixin.CheckName(name);

            AssemblyNameReference reference;
            if (TryGetAssemblyNameReference(name, out reference))
                return reference;

            reference = new(name.Name, name.Version)
            {
                PublicKeyToken = name.GetPublicKeyToken(),
                Culture = name.CultureInfo.Name,
                HashAlgorithm = (AssemblyHashAlgorithm)name.HashAlgorithm
            };

            module.AssemblyReferences.Add(reference);
            return reference;
        }

        private bool TryGetAssemblyNameReference(SR.AssemblyName name, out AssemblyNameReference assembly_reference)
        {
            Collection<AssemblyNameReference> references = module.AssemblyReferences;

            for (int i = 0; i < references.Count; i++)
            {
                AssemblyNameReference reference = references[i];
                if (name.FullName != reference.FullName) // TODO compare field by field
                    continue;

                assembly_reference = reference;
                return true;
            }

            assembly_reference = null;
            return false;
        }

        private FieldReference ImportField(SR.FieldInfo field, ImportGenericContext context)
        {
            TypeReference declaring_type = ImportType(field.DeclaringType, context);

            if (IsGenericInstance(field.DeclaringType))
                field = ResolveFieldDefinition(field);

            context.Push(declaring_type);
            try
            {
                return new()
                {
                    Name = field.Name,
                    DeclaringType = declaring_type,
                    FieldType = ImportType(field.FieldType, context)
                };
            }
            finally
            {
                context.Pop();
            }
        }

        private static SR.FieldInfo ResolveFieldDefinition(SR.FieldInfo field)
        {
            return field.Module.ResolveField(field.MetadataToken);
        }

        private static SR.MethodBase ResolveMethodDefinition(SR.MethodBase method)
        {
            return method.Module.ResolveMethod(method.MetadataToken);
        }

        private MethodReference ImportMethod(SR.MethodBase method, ImportGenericContext context, ImportGenericKind import_kind)
        {
            if (IsMethodSpecification(method) || ImportOpenGenericMethod(method, import_kind))
                return ImportMethodSpecification(method, context);

            TypeReference declaring_type = ImportType(method.DeclaringType, context);

            if (IsGenericInstance(method.DeclaringType))
                method = ResolveMethodDefinition(method);

            MethodReference reference = new()
            {
                Name = method.Name,
                HasThis = HasCallingConvention(method, SR.CallingConventions.HasThis),
                ExplicitThis = HasCallingConvention(method, SR.CallingConventions.ExplicitThis),
                DeclaringType = ImportType(method.DeclaringType, context, ImportGenericKind.Definition)
            };

            if (HasCallingConvention(method, SR.CallingConventions.VarArgs))
                reference.CallingConvention &= MethodCallingConvention.VarArg;

            if (method.IsGenericMethod)
                ImportGenericParameters(reference, method.GetGenericArguments());

            context.Push(reference);
            try
            {
                SR.MethodInfo method_info = method as SR.MethodInfo;
                reference.ReturnType = method_info != null ? ImportType(method_info.ReturnType, context) : ImportType(typeof(void), default);

                SR.ParameterInfo[] parameters = method.GetParameters();
                Collection<ParameterDefinition> reference_parameters = reference.Parameters;

                for (int i = 0; i < parameters.Length; i++)
                    reference_parameters.Add(new(ImportType(parameters[i].ParameterType, context)));

                reference.DeclaringType = declaring_type;

                return reference;
            }
            finally
            {
                context.Pop();
            }
        }

        private static void ImportGenericParameters(IGenericParameterProvider provider, Type[] arguments)
        {
            Collection<GenericParameter> provider_parameters = provider.GenericParameters;

            for (int i = 0; i < arguments.Length; i++)
                provider_parameters.Add(new(arguments[i].Name, provider));
        }

        private static bool IsMethodSpecification(SR.MethodBase method)
        {
            return method.IsGenericMethod && !method.IsGenericMethodDefinition;
        }

        private MethodReference ImportMethodSpecification(SR.MethodBase method, ImportGenericContext context)
        {
            SR.MethodInfo method_info = method as SR.MethodInfo;
            if (method_info == null)
                throw new InvalidOperationException();

            MethodReference element_method = ImportMethod(method_info.GetGenericMethodDefinition(), context, ImportGenericKind.Definition);
            GenericInstanceMethod instance = new(element_method);
            Type[] arguments = method.GetGenericArguments();
            Collection<TypeReference> instance_arguments = instance.GenericArguments;

            context.Push(element_method);
            try
            {
                for (int i = 0; i < arguments.Length; i++)
                    instance_arguments.Add(ImportType(arguments[i], context));

                return instance;
            }
            finally
            {
                context.Pop();
            }
        }

        private static bool HasCallingConvention(SR.MethodBase method, SR.CallingConventions conventions)
        {
            return (method.CallingConvention & conventions) != 0;
        }

        public virtual TypeReference ImportReference(Type type, IGenericParameterProvider context)
        {
            Mixin.CheckType(type);
            return ImportType(type, ImportGenericContext.For(context), context != null ? ImportGenericKind.Open : ImportGenericKind.Definition);
        }

        public virtual FieldReference ImportReference(SR.FieldInfo field, IGenericParameterProvider context)
        {
            Mixin.CheckField(field);
            return ImportField(field, ImportGenericContext.For(context));
        }

        public virtual MethodReference ImportReference(SR.MethodBase method, IGenericParameterProvider context)
        {
            Mixin.CheckMethod(method);
            return ImportMethod(method, ImportGenericContext.For(context), context != null ? ImportGenericKind.Open : ImportGenericKind.Definition);
        }
    }

    public class DefaultMetadataImporter : IMetadataImporter
    {
        protected readonly ModuleDefinition module;

        public DefaultMetadataImporter(ModuleDefinition module)
        {
            Mixin.CheckModule(module);

            this.module = module;
        }

        private TypeReference ImportType(TypeReference type, ImportGenericContext context)
        {
            if (type.IsTypeSpecification())
                return ImportTypeSpecification(type, context);

            TypeReference reference = new(type.Namespace, type.Name, module, ImportScope(type), type.IsValueType);

            MetadataSystem.TryProcessPrimitiveTypeReference(reference);

            if (type.IsNested)
                reference.DeclaringType = ImportType(type.DeclaringType, context);

            if (type.HasGenericParameters)
                ImportGenericParameters(reference, type);

            return reference;
        }

        protected virtual IMetadataScope ImportScope(TypeReference type)
        {
            return ImportScope(type.Scope);
        }

        protected IMetadataScope ImportScope(IMetadataScope scope)
        {
            switch (scope.MetadataScopeType)
            {
                case MetadataScopeType.AssemblyNameReference:
                    return ImportReference((AssemblyNameReference)scope);
                case MetadataScopeType.ModuleDefinition:
                    if (scope == module)
                        return scope;
                    return ImportReference(((ModuleDefinition)scope).Assembly.Name);
                case MetadataScopeType.ModuleReference:
                    throw new NotImplementedException();
            }

            throw new NotSupportedException();
        }

        public virtual AssemblyNameReference ImportReference(AssemblyNameReference name)
        {
            Mixin.CheckName(name);

            AssemblyNameReference reference;
            if (module.TryGetAssemblyNameReference(name, out reference))
                return reference;

            reference = new(name.Name, name.Version)
            {
                Culture = name.Culture,
                HashAlgorithm = name.HashAlgorithm,
                IsRetargetable = name.IsRetargetable,
                IsWindowsRuntime = name.IsWindowsRuntime
            };

            byte[] pk_token = !name.PublicKeyToken.IsNullOrEmpty() ? new byte[name.PublicKeyToken.Length] : Empty<byte>.Array;

            if (pk_token.Length > 0)
                Buffer.BlockCopy(name.PublicKeyToken, 0, pk_token, 0, pk_token.Length);

            reference.PublicKeyToken = pk_token;

            // Only add if not self.
            if (CanAddAssemblyNameReference(module, reference))
                module.AssemblyReferences.Add(reference);

            return reference;
        }

        private bool CanAddAssemblyNameReference(ModuleDefinition module, AssemblyNameReference nameRef)
        {
            return true;
            // return (module.assembly.FullName != nameRef.FullName);
        }

        private static void ImportGenericParameters(IGenericParameterProvider imported, IGenericParameterProvider original)
        {
            Collection<GenericParameter> parameters = original.GenericParameters;
            Collection<GenericParameter> imported_parameters = imported.GenericParameters;

            for (int i = 0; i < parameters.Count; i++)
                imported_parameters.Add(new(parameters[i].Name, imported));
        }

        private TypeReference ImportTypeSpecification(TypeReference type, ImportGenericContext context)
        {
            switch (type.etype)
            {
                case ElementType.SzArray:
                    ArrayType vector = (ArrayType)type;
                    return new ArrayType(ImportType(vector.ElementType, context));
                case ElementType.Ptr:
                    PointerType pointer = (PointerType)type;
                    return new PointerType(ImportType(pointer.ElementType, context));
                case ElementType.ByRef:
                    ByReferenceType byref = (ByReferenceType)type;
                    return new ByReferenceType(ImportType(byref.ElementType, context));
                case ElementType.Pinned:
                    PinnedType pinned = (PinnedType)type;
                    return new PinnedType(ImportType(pinned.ElementType, context));
                case ElementType.Sentinel:
                    SentinelType sentinel = (SentinelType)type;
                    return new SentinelType(ImportType(sentinel.ElementType, context));
                case ElementType.FnPtr:
                    FunctionPointerType fnptr = (FunctionPointerType)type;
                    FunctionPointerType imported_fnptr = new()
                    {
                        HasThis = fnptr.HasThis,
                        ExplicitThis = fnptr.ExplicitThis,
                        CallingConvention = fnptr.CallingConvention,
                        ReturnType = ImportType(fnptr.ReturnType, context)
                    };

                    if (!fnptr.HasParameters)
                        return imported_fnptr;

                    for (int i = 0; i < fnptr.Parameters.Count; i++)
                        imported_fnptr.Parameters.Add(new(ImportType(fnptr.Parameters[i].ParameterType, context)));

                    return imported_fnptr;
                case ElementType.CModOpt:
                    OptionalModifierType modopt = (OptionalModifierType)type;
                    return new OptionalModifierType(ImportType(modopt.ModifierType, context), ImportType(modopt.ElementType, context));
                case ElementType.CModReqD:
                    RequiredModifierType modreq = (RequiredModifierType)type;
                    return new RequiredModifierType(ImportType(modreq.ModifierType, context), ImportType(modreq.ElementType, context));
                case ElementType.Array:
                    ArrayType array = (ArrayType)type;
                    ArrayType imported_array = new(ImportType(array.ElementType, context));
                    if (array.IsVector)
                        return imported_array;

                    Collection<ArrayDimension> dimensions = array.Dimensions;
                    Collection<ArrayDimension> imported_dimensions = imported_array.Dimensions;

                    imported_dimensions.Clear();

                    for (int i = 0; i < dimensions.Count; i++)
                    {
                        ArrayDimension dimension = dimensions[i];

                        imported_dimensions.Add(new(dimension.LowerBound, dimension.UpperBound));
                    }

                    return imported_array;
                case ElementType.GenericInst:
                    GenericInstanceType instance = (GenericInstanceType)type;
                    TypeReference element_type = ImportType(instance.ElementType, context);
                    Collection<TypeReference> arguments = instance.GenericArguments;
                    GenericInstanceType imported_instance = new(element_type, arguments.Count);
                    Collection<TypeReference> imported_arguments = imported_instance.GenericArguments;

                    for (int i = 0; i < arguments.Count; i++)
                        imported_arguments.Add(ImportType(arguments[i], context));

                    return imported_instance;
                case ElementType.Var:
                    GenericParameter var_parameter = (GenericParameter)type;
                    if (var_parameter.DeclaringType == null)
                        throw new InvalidOperationException();
                    return context.TypeParameter(var_parameter.DeclaringType.FullName, var_parameter.Position);
                case ElementType.MVar:
                    GenericParameter mvar_parameter = (GenericParameter)type;
                    if (mvar_parameter.DeclaringMethod == null)
                        throw new InvalidOperationException();
                    return context.MethodParameter(context.NormalizeMethodName(mvar_parameter.DeclaringMethod), mvar_parameter.Position);
            }

            throw new NotSupportedException(type.etype.ToString());
        }

        private FieldReference ImportField(FieldReference field, ImportGenericContext context)
        {
            TypeReference declaring_type = ImportType(field.DeclaringType, context);

            context.Push(declaring_type);
            try
            {
                return new()
                {
                    Name = field.Name,
                    DeclaringType = declaring_type,
                    FieldType = ImportType(field.FieldType, context)
                };
            }
            finally
            {
                context.Pop();
            }
        }

        private MethodReference ImportMethod(MethodReference method, ImportGenericContext context)
        {
            if (method.IsGenericInstance)
                return ImportMethodSpecification(method, context);

            TypeReference declaring_type = ImportType(method.DeclaringType, context);

            MethodReference reference = new()
            {
                Name = method.Name,
                HasThis = method.HasThis,
                ExplicitThis = method.ExplicitThis,
                DeclaringType = declaring_type,
                CallingConvention = method.CallingConvention
            };

            if (method.HasGenericParameters)
                ImportGenericParameters(reference, method);

            context.Push(reference);
            try
            {
                reference.ReturnType = ImportType(method.ReturnType, context);

                if (!method.HasParameters)
                    return reference;

                Collection<ParameterDefinition> parameters = method.Parameters;
                ParameterDefinitionCollection reference_parameters = reference.parameters = new(reference, parameters.Count);
                for (int i = 0; i < parameters.Count; i++)
                    reference_parameters.Add(new(ImportType(parameters[i].ParameterType, context)));

                return reference;
            }
            finally
            {
                context.Pop();
            }
        }

        private MethodSpecification ImportMethodSpecification(MethodReference method, ImportGenericContext context)
        {
            if (!method.IsGenericInstance)
                throw new NotSupportedException();

            GenericInstanceMethod instance = (GenericInstanceMethod)method;
            MethodReference element_method = ImportMethod(instance.ElementMethod, context);
            GenericInstanceMethod imported_instance = new(element_method);

            Collection<TypeReference> arguments = instance.GenericArguments;
            Collection<TypeReference> imported_arguments = imported_instance.GenericArguments;

            for (int i = 0; i < arguments.Count; i++)
                imported_arguments.Add(ImportType(arguments[i], context));

            return imported_instance;
        }

        public virtual TypeReference ImportReference(TypeReference type, IGenericParameterProvider context)
        {
            Mixin.CheckType(type);
            return ImportType(type, ImportGenericContext.For(context));
        }

        public virtual FieldReference ImportReference(FieldReference field, IGenericParameterProvider context)
        {
            Mixin.CheckField(field);
            return ImportField(field, ImportGenericContext.For(context));
        }

        public virtual MethodReference ImportReference(MethodReference method, IGenericParameterProvider context)
        {
            Mixin.CheckMethod(method);
            return ImportMethod(method, ImportGenericContext.For(context));
        }
    }

    internal static partial class Mixin
    {
        public static void CheckModule(ModuleDefinition module)
        {
            if (module == null)
                throw new ArgumentNullException(Argument.module.ToString());
        }

        public static bool TryGetAssemblyNameReference(this ModuleDefinition module, AssemblyNameReference name_reference, out AssemblyNameReference assembly_reference)
        {
            Collection<AssemblyNameReference> references = module.AssemblyReferences;

            for (int i = 0; i < references.Count; i++)
            {
                AssemblyNameReference reference = references[i];
                if (!Equals(name_reference, reference))
                    continue;

                assembly_reference = reference;
                return true;
            }

            assembly_reference = null;
            return false;
        }

        private static bool Equals(byte[] a, byte[] b)
        {
            if (ReferenceEquals(a, b))
                return true;
            if (a == null)
                return false;
            if (a.Length != b.Length)
                return false;
            for (int i = 0; i < a.Length; i++)
                if (a[i] != b[i])
                    return false;
            return true;
        }

        private static bool Equals<T>(T a, T b) where T : class, IEquatable<T>
        {
            if (ReferenceEquals(a, b))
                return true;
            if (a == null)
                return false;
            return a.Equals(b);
        }

        private static bool Equals(AssemblyNameReference a, AssemblyNameReference b)
        {
            if (ReferenceEquals(a, b))
                return true;
            if (a.Name != b.Name)
                return false;
            if (!Equals(a.Version, b.Version))
                return false;
            if (a.Culture != b.Culture)
                return false;
            if (!Equals(a.PublicKeyToken, b.PublicKeyToken))
                return false;
            return true;
        }
    }
}