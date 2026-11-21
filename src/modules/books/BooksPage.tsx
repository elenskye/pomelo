import { CollectionPage } from "@/shared/collection/CollectionPage";
import { BOOKS, booksApi } from "./config";

export default function BooksPage() {
  return <CollectionPage config={BOOKS} api={booksApi} />;
}
