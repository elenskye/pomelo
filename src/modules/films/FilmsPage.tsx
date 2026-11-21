import { CollectionPage } from "@/shared/collection/CollectionPage";
import { FILMS, filmsApi } from "./config";

export default function FilmsPage() {
  return <CollectionPage config={FILMS} api={filmsApi} />;
}
